use std::io::{BufReader, Read};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, Command};

use crate::audio::{AudioSink, PulseSink};

const V: u16 = protocol::MAX_VERSION;
const TENTH_OF_A_SECOND: usize = 4800 * 2;

#[derive(Clone, Copy)]
enum After {
    Serve,
    StopReading,
    HangUp,
    Underflow,
}

fn handshake(r: &mut BufReader<UnixStream>, w: &mut UnixStream) -> Option<()> {
    let (seq, _) = protocol::read_command_message(r, V).ok()?;
    let auth = protocol::AuthReply {
        version: V,
        ..Default::default()
    };
    protocol::write_reply_message(w, seq, &auth, V).ok()?;
    let (seq, _) = protocol::read_command_message(r, V).ok()?;
    protocol::write_reply_message(w, seq, &protocol::SetClientNameReply { client_id: 1 }, V)
        .ok()?;
    let (seq, _) = protocol::read_command_message(r, V).ok()?;
    let stream = protocol::CreatePlaybackStreamReply {
        channel: 3,
        requested_bytes: 1 << 30,
        ..Default::default()
    };
    protocol::write_reply_message(w, seq, &stream, V).ok()
}

fn fake_server(after: After) -> (PulseSink, Receiver<Command>) {
    let (client, server) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || -> Option<()> {
        let mut w = server.try_clone().ok()?;
        let mut r = BufReader::new(server);
        handshake(&mut r, &mut w)?;
        match after {
            After::HangUp => return None,
            After::StopReading => std::thread::sleep(Duration::from_secs(60)),
            After::Underflow => {
                let underflow = Command::Underflow(protocol::Underflow {
                    channel: 3,
                    offset: 9600,
                });
                protocol::write_command_message(&mut w, u32::MAX, &underflow, V).ok()?;
            }
            After::Serve => {}
        }
        loop {
            let desc = protocol::read_descriptor(&mut r).ok()?;
            let mut body = (&mut r).take(desc.length as u64);
            if desc.channel != u32::MAX {
                std::io::copy(&mut body, &mut std::io::sink()).ok()?;
                continue;
            }
            let (seq, cmd) = Command::read_tag_prefixed(&mut body, V).ok()?;
            if matches!(cmd, Command::DrainPlaybackStream(_)) {
                protocol::write_command_message(&mut w, seq, &Command::Reply, V).ok()?;
            }
            tx.send(cmd).ok()?;
        }
    });
    let sink = PulseSink::start_with("engine-test".into(), None, Box::new(move || Ok(client)));
    (sink, rx)
}

fn within(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn flush_and_cork_never_wait_on_a_stalled_server() {
    let (mut sink, _server) = fake_server(After::StopReading);
    for _ in 0..200 {
        sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    }
    std::thread::sleep(Duration::from_millis(300));
    let (tx, rx) = mpsc::channel();
    let control = std::thread::spawn(move || {
        sink.flush();
        sink.set_paused(true);
        tx.send(()).unwrap();
        sink
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(500)).is_ok(),
        "flush/cork blocked behind a socket write the server is not reading"
    );
    drop(control.join().unwrap());
}

#[test]
fn drain_completes_on_the_server_ack_and_a_flush_cancels_it() {
    let (mut sink, server) = fake_server(After::Serve);
    sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    assert!(!sink.drained());
    sink.drain();
    assert!(
        within(Duration::from_secs(2), || sink.drained()),
        "no drain ack"
    );
    assert!(
        server
            .try_iter()
            .any(|c| matches!(c, Command::DrainPlaybackStream(3)))
    );
    assert_eq!(sink.output_position_us(), Some(100_000));
    sink.flush();
    assert!(!sink.drained());
}

#[test]
fn a_lost_connection_marks_the_sink_failed() {
    let (mut sink, _server) = fake_server(After::HangUp);
    sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    assert!(within(Duration::from_secs(2), || sink.failed()));
}

#[test]
fn dropping_a_corked_sink_closes_its_connection() {
    let (mut sink, server) = fake_server(After::Serve);
    sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    sink.set_paused(true);
    std::thread::sleep(Duration::from_millis(100));
    drop(sink);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match server.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(_) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("the server connection outlived the sink"),
        }
    }
}

#[test]
fn a_named_sink_the_server_does_not_have_fails_before_any_stream_exists() {
    let (client, server) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || -> Option<()> {
        let mut w = server.try_clone().ok()?;
        let mut r = BufReader::new(server);
        for _ in 0..3 {
            let (seq, cmd) = protocol::read_command_message(&mut r, V).ok()?;
            match &cmd {
                Command::Auth(_) => {
                    let auth = protocol::AuthReply {
                        version: V,
                        ..Default::default()
                    };
                    protocol::write_reply_message(&mut w, seq, &auth, V).ok()?
                }
                Command::GetSinkInfo(_) => {
                    protocol::write_error(&mut w, seq, &protocol::PulseError::NoEntity).ok()?
                }
                _ => protocol::write_reply_message(
                    &mut w,
                    seq,
                    &protocol::SetClientNameReply { client_id: 1 },
                    V,
                )
                .ok()?,
            }
            tx.send(cmd).ok()?;
        }
        None
    });
    let device = Some("missing".to_owned());
    let mut sink =
        PulseSink::start_with("engine-test".into(), device, Box::new(move || Ok(client)));
    sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    assert!(within(Duration::from_secs(2), || sink.failed()));
    let sent: Vec<Command> = rx.try_iter().collect();
    assert!(
        matches!(sent.last(), Some(Command::GetSinkInfo(_))),
        "{sent:?}"
    );
    assert!(
        !sent
            .iter()
            .any(|c| matches!(c, Command::CreatePlaybackStream(_)))
    );
}

#[test]
fn an_underflow_the_server_sends_is_counted_and_is_not_a_failure() {
    let (mut sink, _server) = fake_server(After::Underflow);
    sink.write(0, 48_000, 2, vec![0; TENTH_OF_A_SECOND]);
    let mut seen = 0;
    assert!(
        within(Duration::from_secs(2), || {
            seen += sink.take_output_underflows();
            seen > 0
        }),
        "no underflow counted"
    );
    std::thread::sleep(Duration::from_millis(100));
    seen += sink.take_output_underflows();
    assert_eq!((seen, sink.failed()), (1, false));
}
