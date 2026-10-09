use std::ffi::CString;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use pulseaudio::protocol;

pub struct Pcm {
    pub generation: u64,
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

pub struct AudioOut {
    pub tx: SyncSender<Pcm>,
    pub generation: Arc<AtomicU64>,
    pub played_frames: Arc<AtomicU64>,
    pub stream_created: Arc<AtomicBool>,
    pub sent_frames: u64,
}

impl AudioOut {
    pub fn start(sink: String) -> AudioOut {
        let (tx, rx) = sync_channel::<Pcm>(256);
        let generation = Arc::new(AtomicU64::new(0));
        let played_frames = Arc::new(AtomicU64::new(0));
        let stream_created = Arc::new(AtomicBool::new(false));
        let g = generation.clone();
        let p = played_frames.clone();
        let s = stream_created.clone();
        std::thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || {
                if let Err(e) = run(sink, rx, g, p, s) {
                    eprintln!("ENGINE audio thread error: {e:#}");
                }
            })
            .expect("spawn audio thread");
        AudioOut { tx, generation, played_frames, stream_created, sent_frames: 0 }
    }

    pub fn flush(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.played_frames.store(0, Ordering::SeqCst);
        self.sent_frames = 0;
    }

    pub fn played(&self) -> u64 {
        self.played_frames.load(Ordering::SeqCst)
    }

    pub fn queued(&self) -> u64 {
        self.sent_frames.saturating_sub(self.played())
    }

    pub fn send(&mut self, rate: u32, channels: u16, samples: Vec<i16>) -> bool {
        let frames = samples.len() as u64 / channels.max(1) as u64;
        let generation = self.generation.load(Ordering::SeqCst);
        if self.tx.send(Pcm { generation, rate, channels, samples }).is_err() {
            return false;
        }
        self.sent_frames += frames;
        true
    }
}

fn run(sink: String, rx: Receiver<Pcm>, generation: Arc<AtomicU64>, played: Arc<AtomicU64>, created: Arc<AtomicBool>) -> Result<()> {
    let first = loop {
        let pcm = rx.recv().context("audio channel closed before first chunk")?;
        if pcm.generation == generation.load(Ordering::SeqCst) {
            break pcm;
        }
    };
    let rate = first.rate;
    let channels = first.channels;
    let socket_path = pulseaudio::socket_path_from_env().context("PulseAudio socket not found")?;
    let mut sock = BufReader::new(UnixStream::connect(socket_path)?);
    let cookie = pulseaudio::cookie_path_from_env().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    protocol::write_command_message(sock.get_mut(), 0, &protocol::Command::Auth(protocol::AuthParams { version: protocol::MAX_VERSION, supports_shm: false, supports_memfd: false, cookie }), protocol::MAX_VERSION)?;
    let (_, auth) = protocol::read_reply_message::<protocol::AuthReply>(&mut sock, protocol::MAX_VERSION)?;
    let version = std::cmp::min(protocol::MAX_VERSION, auth.version);
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, CString::new("spike5-engine").unwrap());
    props.set(protocol::Prop::ApplicationProcessId, CString::new(std::process::id().to_string()).unwrap());
    protocol::write_command_message(sock.get_mut(), 1, &protocol::Command::SetClientName(props), version)?;
    let _ = protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, version)?;
    let bytes_per_sec = rate * channels as u32 * 2;
    let channel_map = if channels == 1 { protocol::ChannelMap::mono() } else { protocol::ChannelMap::stereo() };
    protocol::write_command_message(
        sock.get_mut(),
        2,
        &protocol::Command::CreatePlaybackStream(protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec { format: protocol::SampleFormat::S16Le, channels: channels as u8, sample_rate: rate },
            channel_map,
            cvolume: Some(protocol::ChannelVolume::norm(channels as u8)),
            sink_name: Some(CString::new(sink).unwrap()),
            buffer_attr: protocol::stream::BufferAttr { max_length: u32::MAX, target_length: bytes_per_sec / 5, pre_buffering: u32::MAX, minimum_request_length: u32::MAX, fragment_size: u32::MAX },
            ..Default::default()
        }),
        version,
    )?;
    let (_, info) = protocol::read_reply_message::<protocol::CreatePlaybackStreamReply>(&mut sock, version)?;
    created.store(true, Ordering::SeqCst);
    eprintln!("ENGINE audio stream created rate={rate} channels={channels} channel={} sink_index={}", info.channel, info.sink_index);
    let frame_bytes = channels as u64 * 2;
    let mut pending: Vec<u8> = first.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut pending_gen = first.generation;
    let fill = |want: usize, pending: &mut Vec<u8>, pending_gen: &mut u64| -> Option<Vec<u8>> {
        loop {
            let current = generation.load(Ordering::SeqCst);
            if *pending_gen != current {
                pending.clear();
                *pending_gen = current;
            }
            if pending.len() >= want {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(chunk) => {
                    if chunk.generation != generation.load(Ordering::SeqCst) {
                        continue;
                    }
                    if chunk.generation != *pending_gen {
                        pending.clear();
                        *pending_gen = chunk.generation;
                    }
                    pending.extend(chunk.samples.iter().flat_map(|s| s.to_le_bytes()));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if pending.len() >= frame_bytes as usize {
                        break;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if pending.is_empty() {
            return None;
        }
        let n = want.min(pending.len());
        let n = n - n % frame_bytes as usize;
        Some(pending.drain(..n).collect())
    };
    let write = |sock: &mut BufReader<UnixStream>, buf: Vec<u8>, gen_at_fill: u64| -> Result<()> {
        protocol::write_memblock(sock.get_mut(), info.channel, &buf, 0)?;
        if gen_at_fill == generation.load(Ordering::SeqCst) {
            played.fetch_add(buf.len() as u64 / frame_bytes, Ordering::SeqCst);
        }
        Ok(())
    };
    if let Some(buf) = fill(info.requested_bytes as usize, &mut pending, &mut pending_gen) {
        write(&mut sock, buf, pending_gen)?;
    }
    loop {
        let (_, msg) = protocol::read_command_message(&mut sock, version)?;
        match msg {
            protocol::Command::Request(protocol::Request { channel, length }) if channel == info.channel => match fill(length as usize, &mut pending, &mut pending_gen) {
                Some(buf) => write(&mut sock, buf, pending_gen)?,
                None => return Ok(()),
            },
            protocol::Command::Error(e) => bail!("pulse error {e:?}"),
            _ => {}
        }
    }
}
