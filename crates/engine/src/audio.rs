pub trait AudioSink {
    fn write(&mut self, pts_us: i64, rate: u32, channels: u16, samples: Vec<i16>);
    fn played_frames(&self) -> u64;
    fn output_position_us(&self) -> Option<i64>;
    fn flush(&mut self);
    fn set_paused(&mut self, paused: bool);
    fn set_gain(&mut self, gain: f32);
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
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use anyhow::{Context, Result, bail};
    use pulseaudio::protocol::{self, Command};

    use super::{AudioSink, apply_gain};

    struct Pcm {
        generation: u64,
        pts_us: i64,
        rate: u32,
        channels: u16,
        samples: Vec<i16>,
    }

    struct Control {
        sock: UnixStream,
        channel: u32,
        version: u16,
        seq: u32,
    }

    impl Control {
        fn send(&mut self, cmd: &Command) {
            self.seq += 1;
            if let Err(e) =
                protocol::write_command_message(&mut self.sock, self.seq, cmd, self.version)
            {
                eprintln!("ENGINE audio control write failed: {e}");
            }
        }
    }

    struct Shared {
        generation: AtomicU64,
        played: AtomicU64,
        position_us: AtomicI64,
        gain: AtomicU32,
        paused: AtomicBool,
        control: Mutex<Option<Control>>,
    }

    impl Shared {
        fn control(&self, f: impl FnOnce(&mut Control)) {
            if let Some(c) = self.control.lock().unwrap().as_mut() {
                f(c);
            }
        }
    }

    pub struct PulseSink {
        tx: SyncSender<Pcm>,
        shared: Arc<Shared>,
    }

    impl PulseSink {
        pub fn start(app_name: String, device: Option<String>) -> PulseSink {
            let (tx, rx) = sync_channel::<Pcm>(256);
            let shared = Arc::new(Shared {
                generation: AtomicU64::new(0),
                played: AtomicU64::new(0),
                position_us: AtomicI64::new(i64::MIN),
                gain: AtomicU32::new(1f32.to_bits()),
                paused: AtomicBool::new(false),
                control: Mutex::new(None),
            });
            let s = shared.clone();
            std::thread::Builder::new()
                .name("audio-out".into())
                .spawn(move || {
                    if let Err(e) = run(app_name, device, rx, s) {
                        eprintln!("ENGINE audio thread error: {e:#}");
                    }
                })
                .expect("spawn audio thread");
            PulseSink { tx, shared }
        }
    }

    impl AudioSink for PulseSink {
        fn write(&mut self, pts_us: i64, rate: u32, channels: u16, samples: Vec<i16>) {
            let generation = self.shared.generation.load(Ordering::SeqCst);
            let _ = self.tx.send(Pcm {
                generation,
                pts_us,
                rate,
                channels,
                samples,
            });
        }
        fn played_frames(&self) -> u64 {
            self.shared.played.load(Ordering::SeqCst)
        }
        fn output_position_us(&self) -> Option<i64> {
            Some(self.shared.position_us.load(Ordering::SeqCst)).filter(|p| *p != i64::MIN)
        }
        fn flush(&mut self) {
            self.shared.generation.fetch_add(1, Ordering::SeqCst);
            self.shared.control(|c| {
                let channel = c.channel;
                c.send(&Command::FlushPlaybackStream(channel))
            });
        }
        fn set_paused(&mut self, paused: bool) {
            self.shared.paused.store(paused, Ordering::SeqCst);
            self.shared.control(|c| {
                let channel = c.channel;
                c.send(&Command::CorkPlaybackStream(protocol::CorkStreamParams {
                    channel,
                    cork: paused,
                }))
            });
        }
        fn set_gain(&mut self, gain: f32) {
            self.shared.gain.store(gain.to_bits(), Ordering::SeqCst);
        }
    }

    struct Pending {
        bytes: Vec<u8>,
        generation: u64,
        head_pts_us: i64,
    }

    fn run(
        app_name: String,
        device: Option<String>,
        rx: Receiver<Pcm>,
        shared: Arc<Shared>,
    ) -> Result<()> {
        let first = loop {
            let pcm = rx
                .recv()
                .context("audio channel closed before first chunk")?;
            if pcm.generation == shared.generation.load(Ordering::SeqCst) {
                break pcm;
            }
        };
        let (rate, channels) = (first.rate, first.channels);
        let socket_path =
            pulseaudio::socket_path_from_env().context("PulseAudio socket not found")?;
        let mut sock = BufReader::new(UnixStream::connect(socket_path)?);
        let cookie = pulseaudio::cookie_path_from_env()
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        protocol::write_command_message(
            sock.get_mut(),
            0,
            &Command::Auth(protocol::AuthParams {
                version: protocol::MAX_VERSION,
                supports_shm: false,
                supports_memfd: false,
                cookie,
            }),
            protocol::MAX_VERSION,
        )?;
        let (_, auth) =
            protocol::read_reply_message::<protocol::AuthReply>(&mut sock, protocol::MAX_VERSION)?;
        let version = std::cmp::min(protocol::MAX_VERSION, auth.version);
        let mut props = protocol::Props::new();
        props.set(protocol::Prop::ApplicationName, CString::new(app_name)?);
        props.set(
            protocol::Prop::ApplicationProcessId,
            CString::new(std::process::id().to_string())?,
        );
        protocol::write_command_message(
            sock.get_mut(),
            1,
            &Command::SetClientName(props),
            version,
        )?;
        let _ = protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, version)?;
        let bytes_per_sec = rate * channels as u32 * 2;
        let channel_map = if channels == 1 {
            protocol::ChannelMap::mono()
        } else {
            protocol::ChannelMap::stereo()
        };
        let params = protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec {
                format: protocol::SampleFormat::S16Le,
                channels: channels as u8,
                sample_rate: rate,
            },
            channel_map,
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
        };
        protocol::write_command_message(
            sock.get_mut(),
            2,
            &Command::CreatePlaybackStream(params),
            version,
        )?;
        let (_, info) = protocol::read_reply_message::<protocol::CreatePlaybackStreamReply>(
            &mut sock, version,
        )?;
        eprintln!(
            "ENGINE audio stream created rate={rate} channels={channels} channel={} sink_index={}",
            info.channel, info.sink_index
        );
        *shared.control.lock().unwrap() = Some(Control {
            sock: sock.get_ref().try_clone()?,
            channel: info.channel,
            version,
            seq: 2,
        });
        if shared.paused.load(Ordering::SeqCst) {
            shared.control(|c| {
                c.send(&Command::CorkPlaybackStream(protocol::CorkStreamParams {
                    channel: info.channel,
                    cork: true,
                }))
            });
        }
        let frame_bytes = channels as usize * 2;
        let mut pending = Pending {
            bytes: Vec::new(),
            generation: first.generation,
            head_pts_us: first.pts_us,
        };
        pending
            .bytes
            .extend(first.samples.iter().flat_map(|s| s.to_le_bytes()));
        let fill = |want: usize, p: &mut Pending| -> Option<(Vec<u8>, u64, i64)> {
            loop {
                let current = shared.generation.load(Ordering::SeqCst);
                if p.generation != current {
                    p.bytes.clear();
                    p.generation = current;
                }
                if p.bytes.len() >= want {
                    break;
                }
                match rx.recv_timeout(Duration::from_millis(40)) {
                    Ok(chunk) if chunk.generation == shared.generation.load(Ordering::SeqCst) => {
                        if chunk.generation != p.generation || p.bytes.is_empty() {
                            p.bytes.clear();
                            p.generation = chunk.generation;
                            p.head_pts_us = chunk.pts_us;
                        }
                        p.bytes
                            .extend(chunk.samples.iter().flat_map(|s| s.to_le_bytes()));
                    }
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) if p.bytes.len() >= frame_bytes => break,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            if p.bytes.is_empty() {
                return None;
            }
            let n = want.min(p.bytes.len());
            let n = n - n % frame_bytes;
            let start_pts = p.head_pts_us;
            p.head_pts_us += (n / frame_bytes) as i64 * 1_000_000 / rate as i64;
            Some((p.bytes.drain(..n).collect(), p.generation, start_pts))
        };
        let write = |(mut buf, generation, start_pts): (Vec<u8>, u64, i64)| -> Result<()> {
            apply_gain(&mut buf, f32::from_bits(shared.gain.load(Ordering::SeqCst)));
            let frames = (buf.len() / frame_bytes) as u64;
            let mut guard = shared.control.lock().unwrap();
            let c = guard.as_mut().context("audio control missing")?;
            protocol::write_memblock(&mut c.sock, c.channel, &buf, 0)?;
            if generation == shared.generation.load(Ordering::SeqCst) {
                shared.played.fetch_add(frames, Ordering::SeqCst);
                shared.position_us.store(
                    start_pts + frames as i64 * 1_000_000 / rate as i64,
                    Ordering::SeqCst,
                );
            }
            Ok(())
        };
        if let Some(chunk) = fill(info.requested_bytes as usize, &mut pending) {
            write(chunk)?;
        }
        loop {
            let (_, msg) = protocol::read_command_message(&mut sock, version)?;
            match msg {
                Command::Request(protocol::Request { channel, length })
                    if channel == info.channel =>
                {
                    match fill(length as usize, &mut pending) {
                        Some(chunk) => write(chunk)?,
                        None => return Ok(()),
                    }
                }
                Command::Error(e) => bail!("pulse error {e:?}"),
                _ => {}
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
}
