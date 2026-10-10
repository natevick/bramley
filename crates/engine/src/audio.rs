pub trait AudioSink {
    fn write(&mut self, pts_us: i64, rate: u32, channels: u16, samples: Vec<i16>);
    fn played_frames(&self) -> u64;
    fn output_position_us(&self) -> Option<i64>;
    fn flush(&mut self);
    fn set_paused(&mut self, paused: bool);
    fn set_gain(&mut self, gain: f32);
    fn drain(&mut self);
    fn drained(&self) -> bool;
    fn failed(&self) -> bool;
}

pub fn apply_gain(pcm_le: &mut [u8], gain: f32) {
    if gain == 1.0 {
        return;
    }
    for s in pcm_le.as_chunks_mut::<2>().0 {
        let v = (i16::from_le_bytes([s[0], s[1]]) as f32 * gain)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        s.copy_from_slice(&v.to_le_bytes());
    }
}

#[cfg(unix)]
pub use pulse::PulseSink;

#[cfg(unix)]
mod pulse {
    use std::ffi::CString;
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering::SeqCst};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};

    use anyhow::{Context, Result, anyhow};
    use pulseaudio::protocol::{self, Command};

    use super::{AudioSink, apply_gain};

    pub type Connect = Box<dyn FnOnce() -> Result<UnixStream> + Send>;

    struct Pcm {
        generation: u64,
        pts_us: i64,
        rate: u32,
        channels: u16,
        samples: Vec<i16>,
    }

    enum Msg {
        Pcm(Pcm),
        Flush,
        Cork(bool),
        Drain(u64),
        Requested(usize),
        Replied(u32),
        ServerGone(String),
        Stop,
    }

    #[derive(Default)]
    struct Shared {
        generation: AtomicU64,
        played: AtomicU64,
        position_us: AtomicI64,
        gain: AtomicU32,
        drained_generation: AtomicU64,
        failed: AtomicBool,
        sock: Mutex<Option<UnixStream>>,
    }

    pub struct PulseSink {
        tx: Sender<Msg>,
        shared: Arc<Shared>,
    }

    impl PulseSink {
        pub fn start(app_name: String, device: Option<String>) -> PulseSink {
            let connect: Connect = Box::new(|| {
                let path =
                    pulseaudio::socket_path_from_env().context("PulseAudio socket not found")?;
                Ok(UnixStream::connect(path)?)
            });
            Self::start_with(app_name, device, connect)
        }

        pub fn start_with(app_name: String, device: Option<String>, connect: Connect) -> PulseSink {
            let (tx, rx) = channel();
            let shared = Arc::new(Shared {
                position_us: AtomicI64::new(i64::MIN),
                gain: AtomicU32::new(1f32.to_bits()),
                drained_generation: AtomicU64::new(u64::MAX),
                ..Default::default()
            });
            let (s, server_tx) = (shared.clone(), tx.clone());
            std::thread::Builder::new()
                .name("audio-out".into())
                .spawn(move || {
                    if let Err(e) = run(app_name, device, connect, rx, server_tx, &s) {
                        eprintln!("ENGINE audio thread error: {e:#}");
                        s.failed.store(true, SeqCst);
                    }
                })
                .expect("spawn audio thread");
            PulseSink { tx, shared }
        }
    }

    impl Drop for PulseSink {
        fn drop(&mut self) {
            let _ = self.tx.send(Msg::Stop);
            if let Some(sock) = self.shared.sock.lock().unwrap().take() {
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    impl AudioSink for PulseSink {
        fn write(&mut self, pts_us: i64, rate: u32, channels: u16, samples: Vec<i16>) {
            let generation = self.shared.generation.load(SeqCst);
            let _ = self.tx.send(Msg::Pcm(Pcm {
                generation,
                pts_us,
                rate,
                channels,
                samples,
            }));
        }
        fn played_frames(&self) -> u64 {
            self.shared.played.load(SeqCst)
        }
        fn output_position_us(&self) -> Option<i64> {
            Some(self.shared.position_us.load(SeqCst)).filter(|p| *p != i64::MIN)
        }
        fn flush(&mut self) {
            self.shared.generation.fetch_add(1, SeqCst);
            let _ = self.tx.send(Msg::Flush);
        }
        fn set_paused(&mut self, paused: bool) {
            let _ = self.tx.send(Msg::Cork(paused));
        }
        fn set_gain(&mut self, gain: f32) {
            self.shared.gain.store(gain.to_bits(), SeqCst);
        }
        fn drain(&mut self) {
            let _ = self
                .tx
                .send(Msg::Drain(self.shared.generation.load(SeqCst)));
        }
        fn drained(&self) -> bool {
            self.shared.drained_generation.load(SeqCst) == self.shared.generation.load(SeqCst)
        }
        fn failed(&self) -> bool {
            self.shared.failed.load(SeqCst)
        }
    }

    struct Stream {
        sock: UnixStream,
        channel: u32,
        version: u16,
        seq: u32,
        rate: u32,
        frame_bytes: usize,
    }

    impl Stream {
        fn send(&mut self, cmd: &Command) -> Result<u32> {
            self.seq += 1;
            protocol::write_command_message(&mut self.sock, self.seq, cmd, self.version)?;
            Ok(self.seq)
        }
        fn cork(&mut self, cork: bool) -> Result<u32> {
            let channel = self.channel;
            self.send(&Command::CorkPlaybackStream(protocol::CorkStreamParams {
                channel,
                cork,
            }))
        }
    }

    fn open(
        app_name: String,
        device: Option<String>,
        connect: Connect,
        first: &Pcm,
        server_tx: Sender<Msg>,
        shared: &Shared,
    ) -> Result<(Stream, usize)> {
        let (rate, channels) = (first.rate, first.channels);
        let sock = connect()?;
        *shared.sock.lock().unwrap() = Some(sock.try_clone()?);
        let mut reader = BufReader::new(sock.try_clone()?);
        let mut stream = Stream {
            sock,
            channel: 0,
            version: protocol::MAX_VERSION,
            seq: 0,
            rate,
            frame_bytes: channels as usize * 2,
        };
        let cookie = pulseaudio::cookie_path_from_env()
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        stream.send(&Command::Auth(protocol::AuthParams {
            version: protocol::MAX_VERSION,
            supports_shm: false,
            supports_memfd: false,
            cookie,
        }))?;
        let (_, auth) =
            protocol::read_reply_message::<protocol::AuthReply>(&mut reader, stream.version)?;
        stream.version = std::cmp::min(protocol::MAX_VERSION, auth.version);
        let mut props = protocol::Props::new();
        props.set(protocol::Prop::ApplicationName, CString::new(app_name)?);
        props.set(
            protocol::Prop::ApplicationProcessId,
            CString::new(std::process::id().to_string())?,
        );
        stream.send(&Command::SetClientName(props))?;
        protocol::read_reply_message::<protocol::SetClientNameReply>(&mut reader, stream.version)?;
        if let Some(wanted) = &device {
            let name = Some(CString::new(wanted.as_str())?);
            stream.send(&Command::GetSinkInfo(protocol::GetSinkInfo {
                index: None,
                name,
            }))?;
            protocol::read_reply_message::<protocol::SinkInfo>(&mut reader, stream.version)
                .with_context(|| format!("output device {wanted} not found"))?;
        }
        let bytes_per_sec = rate * channels as u32 * 2;
        stream.send(&Command::CreatePlaybackStream(
            protocol::PlaybackStreamParams {
                sample_spec: protocol::SampleSpec {
                    format: protocol::SampleFormat::S16Le,
                    channels: channels as u8,
                    sample_rate: rate,
                },
                channel_map: if channels == 1 {
                    protocol::ChannelMap::mono()
                } else {
                    protocol::ChannelMap::stereo()
                },
                cvolume: Some(protocol::ChannelVolume::norm(channels as u8)),
                sink_name: device.map(CString::new).transpose()?,
                buffer_attr: protocol::stream::BufferAttr {
                    max_length: u32::MAX,
                    target_length: bytes_per_sec / 5,
                    pre_buffering: u32::MAX,
                    minimum_request_length: u32::MAX,
                    fragment_size: u32::MAX,
                },
                ..Default::default()
            },
        ))?;
        let (_, info) = protocol::read_reply_message::<protocol::CreatePlaybackStreamReply>(
            &mut reader,
            stream.version,
        )?;
        eprintln!(
            "ENGINE audio stream created rate={rate} channels={channels} channel={} sink_index={}",
            info.channel, info.sink_index
        );
        stream.channel = info.channel;
        let version = stream.version;
        std::thread::Builder::new()
            .name("audio-in".into())
            .spawn(move || {
                loop {
                    let msg = match protocol::read_command_message(&mut reader, version) {
                        Ok((_, Command::Request(r))) => Msg::Requested(r.length as usize),
                        Ok((seq, Command::Reply)) => Msg::Replied(seq),
                        Ok((_, Command::Error(e))) => Msg::ServerGone(format!("{e:?}")),
                        Ok(_) => continue,
                        Err(e) => Msg::ServerGone(e.to_string()),
                    };
                    let gone = matches!(msg, Msg::ServerGone(_));
                    if server_tx.send(msg).is_err() || gone {
                        return;
                    }
                }
            })?;
        Ok((stream, info.requested_bytes as usize))
    }

    struct Pending {
        bytes: Vec<u8>,
        generation: u64,
        head_pts_us: i64,
    }

    fn run(
        app_name: String,
        device: Option<String>,
        connect: Connect,
        rx: Receiver<Msg>,
        server_tx: Sender<Msg>,
        shared: &Shared,
    ) -> Result<()> {
        let mut setup = Some((app_name, device, connect, server_tx));
        let mut stream: Option<Stream> = None;
        let mut pending = Pending {
            bytes: Vec::new(),
            generation: 0,
            head_pts_us: 0,
        };
        let (mut owed, mut corked) = (0usize, false);
        let mut drain_wanted: Option<u64> = None;
        let mut drain_reply: Option<(u32, u64)> = None;
        loop {
            match rx.recv()? {
                Msg::Stop => return Ok(()),
                Msg::Pcm(p) if p.generation == shared.generation.load(SeqCst) => {
                    if let Some((app_name, device, connect, server_tx)) = setup.take() {
                        let (mut s, requested) =
                            open(app_name, device, connect, &p, server_tx, shared)?;
                        if corked {
                            s.cork(true)?;
                        }
                        owed = requested;
                        stream = Some(s);
                    }
                    if p.generation != pending.generation || pending.bytes.is_empty() {
                        pending.bytes.clear();
                        pending.generation = p.generation;
                        pending.head_pts_us = p.pts_us;
                    }
                    pending
                        .bytes
                        .extend(p.samples.iter().flat_map(|s| s.to_le_bytes()));
                }
                Msg::Pcm(_) => {}
                Msg::Flush => {
                    pending.bytes.clear();
                    drain_wanted = None;
                    drain_reply = None;
                    if let Some(s) = stream.as_mut() {
                        let channel = s.channel;
                        s.send(&Command::FlushPlaybackStream(channel))?;
                    }
                }
                Msg::Cork(c) => {
                    corked = c;
                    if let Some(s) = stream.as_mut() {
                        s.cork(c)?;
                    }
                }
                Msg::Drain(generation) => drain_wanted = Some(generation),
                Msg::Requested(n) => owed += n,
                Msg::Replied(seq) => {
                    if let Some((_, generation)) = drain_reply.filter(|(s, _)| *s == seq) {
                        shared.drained_generation.store(generation, SeqCst);
                        drain_reply = None;
                    }
                }
                Msg::ServerGone(e) => return Err(anyhow!("pulse connection lost: {e}")),
            }
            let Some(s) = stream.as_mut() else { continue };
            if pending.generation != shared.generation.load(SeqCst) {
                pending.bytes.clear();
            }
            let n = owed.min(pending.bytes.len());
            let n = n - n % s.frame_bytes;
            if n > 0 {
                let mut buf: Vec<u8> = pending.bytes.drain(..n).collect();
                apply_gain(&mut buf, f32::from_bits(shared.gain.load(SeqCst)));
                protocol::write_memblock(&mut s.sock, s.channel, &buf, 0)?;
                owed -= n;
                let frames = (n / s.frame_bytes) as u64;
                pending.head_pts_us += frames as i64 * 1_000_000 / s.rate as i64;
                shared.played.fetch_add(frames, SeqCst);
                shared.position_us.store(pending.head_pts_us, SeqCst);
            }
            if pending.bytes.is_empty()
                && let Some(generation) = drain_wanted.take()
                && generation == shared.generation.load(SeqCst)
            {
                let channel = s.channel;
                drain_reply = Some((s.send(&Command::DrainPlaybackStream(channel))?, generation));
            }
        }
    }
}

#[cfg(not(unix))]
pub struct PulseSink;

#[cfg(not(unix))]
impl PulseSink {
    pub fn start(_app_name: String, _device: Option<String>) -> NullSink {
        eprintln!("ENGINE audio output is only implemented for unix targets; discarding audio");
        NullSink(0)
    }
}

#[cfg(not(unix))]
pub struct NullSink(u64);

#[cfg(not(unix))]
impl AudioSink for NullSink {
    fn write(&mut self, _pts_us: i64, _rate: u32, channels: u16, samples: Vec<i16>) {
        self.0 += samples.len() as u64 / channels.max(1) as u64;
    }
    fn played_frames(&self) -> u64 {
        self.0
    }
    fn output_position_us(&self) -> Option<i64> {
        None
    }
    fn flush(&mut self) {}
    fn set_paused(&mut self, _paused: bool) {}
    fn set_gain(&mut self, _gain: f32) {}
    fn drain(&mut self) {}
    fn drained(&self) -> bool {
        true
    }
    fn failed(&self) -> bool {
        false
    }
}
