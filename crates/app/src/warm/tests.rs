use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bramley_api::store::Store;
use bramley_api::warm::{Limiter, Report, Stop};
use fastframe_instance::{Claim, Slot};

use super::{fetcher, lock, paused, record, recorded_scale, running, sizes, start};
use crate::covers::{self, Tally};
use crate::tests::mock_user;

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let dir = root
        .join("target/tmp")
        .join(format!("w{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn jpeg() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let image = image::RgbImage::new(4, 4);
    image
        .write_to(&mut bytes, image::ImageFormat::Jpeg)
        .unwrap();
    bytes.into_inner()
}

struct Cdn {
    base: String,
    paths: Arc<Mutex<Vec<String>>>,
}

fn cdn(status: u16) -> Cdn {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let paths = Arc::new(Mutex::new(Vec::new()));
    let seen = paths.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            seen.lock()
                .unwrap()
                .push(line.split_whitespace().nth(1).unwrap().to_owned());
            while reader.read_line(&mut line).unwrap() > 2 && !line.ends_with("\r\n\r\n") {
                line.clear();
            }
            let body = if status == 200 { jpeg() } else { Vec::new() };
            let head = format!(
                "HTTP/1.1 {status} X\r\nRetry-After: 120\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body));
        }
    });
    Cdn { base, paths }
}

fn limiter(budget: usize) -> Arc<Limiter> {
    Arc::new(Limiter::new(budget, Duration::ZERO, || false))
}

fn art_store(name: &str) -> Arc<Store> {
    Arc::new(Store::open(&scratch(name), "art", 1 << 20).unwrap())
}

#[test]
fn warm_exits_at_once_and_does_nothing_when_the_apps_slot_is_held() {
    let dir = scratch("held");
    let slot = Slot::at(&dir, "bramley");
    assert!(!running(&slot));
    let held = slot.claim("show", |_| Some("ok".into()));
    assert!(matches!(held, Claim::First(_)));
    assert!(running(&slot));
    let started = Instant::now();
    let report = super::warm(&["warm".into()], Ok(slot.clone())).unwrap();
    assert_eq!(
        report,
        Report {
            stop: Stop::AppRunning,
            ..Report::default()
        }
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(slot.claim("show", |_| None), Claim::Running(_)));
}

#[test]
fn probing_for_the_app_never_takes_the_slot_from_a_starting_app() {
    let slot = Slot::at(scratch("probe"), "bramley");
    for _ in 0..20 {
        assert!(!running(&slot));
    }
    assert!(matches!(
        slot.claim("show", |_| Some("ok".into())),
        Claim::First(_)
    ));
}

#[test]
fn an_app_start_during_warm_makes_the_limiter_stop_within_seconds() {
    let slot = Slot::at(scratch("during"), "bramley");
    let watching = slot.clone();
    let limiter = Limiter::new(60, Duration::ZERO, move || running(&watching));
    assert!(limiter.spend(true).is_ok());
    let starting = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let guard = slot.claim("show", |_| Some("ok".into()));
        std::thread::sleep(Duration::from_secs(1));
        drop(guard);
    });
    let begun = Instant::now();
    while limiter.spend(true).is_ok() {
        assert!(begun.elapsed() < Duration::from_secs(3), "warm kept going");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(limiter.stopped(), Some(Stop::AppRunning));
    starting.join().unwrap();
}

#[test]
fn two_warms_cannot_overlap() {
    let root = scratch("lock");
    let first = lock(&root).unwrap();
    assert!(first.is_some());
    assert!(lock(&root).unwrap().is_none());
    drop(first);
    assert!(lock(&root).unwrap().is_some());
}

#[test]
fn with_no_token_warm_does_nothing() {
    let dir = scratch("no-token");
    let slot = Slot::at(dir.join("slot"), "bramley");
    let report = start(mock_user(&dir), slot, dir.join("cache")).unwrap();
    assert_eq!(
        report,
        Report {
            stop: Stop::NoToken,
            ..Report::default()
        }
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "no cache directory, no lock"
    );
}

#[test]
fn the_recorded_scale_sizes_the_covers_the_way_the_ui_does() {
    let root = scratch("scale");
    assert_eq!(recorded_scale(&root), None);
    for (text, expected) in [
        ("1.5\n", Some(1.5)),
        ("0", None),
        ("junk", None),
        ("inf", None),
    ] {
        std::fs::write(root.join("scale"), text).unwrap();
        assert_eq!(recorded_scale(&root), expected, "{text}");
    }
    let sizes = sizes(1.5);
    assert_eq!((sizes.cell, sizes.row, sizes.home), (210, 60, 144));
}

#[test]
fn warms_art_is_found_under_the_apps_own_key_without_another_request() {
    let (cdn, store, limiter) = (cdn(200), art_store("keys"), limiter(10));
    let url = format!("{}/art/1/40x40bb.jpg", cdn.base);
    let mut warm = fetcher(store.clone(), limiter.clone(), Some(cdn.base.clone()));
    warm(&url, 40);
    warm(&url, 40);
    assert_eq!(cdn.paths.lock().unwrap().len(), 1);
    let app = Tally::caching(Some(store));
    covers::cover(&mut covers::agent(), &url, 40, &app).unwrap();
    assert_eq!(
        cdn.paths.lock().unwrap().len(),
        1,
        "the app had to fetch it again"
    );
    assert_eq!(limiter.requests(), 1);
}

#[test]
fn art_counts_against_the_budget_and_stops_at_it() {
    let (cdn, store, limiter) = (cdn(200), art_store("budget"), limiter(2));
    let mut warm = fetcher(store, limiter.clone(), Some(cdn.base.clone()));
    let url = |n: usize| format!("{}/art/{n}/40x40bb.jpg", cdn.base);
    (0..4).for_each(|n| warm(&url(n), 40));
    assert_eq!(cdn.paths.lock().unwrap().len(), 2);
    assert_eq!(limiter.stopped(), Some(Stop::Budget));
}

#[test]
fn a_429_5xx_401_or_403_from_the_art_cdn_stops_the_run_and_stores_nothing() {
    for (status, stop) in [
        (429, Stop::RateLimited),
        (500, Stop::ServerError),
        (503, Stop::ServerError),
        (401, Stop::Refused),
        (403, Stop::Refused),
    ] {
        let (cdn, store, limiter) = (
            cdn(status),
            art_store(&format!("art-{status}")),
            limiter(10),
        );
        let mut warm = fetcher(store.clone(), limiter.clone(), Some(cdn.base.clone()));
        let url = |n: usize| format!("{}/art/{n}/40x40bb.jpg", cdn.base);
        warm(&url(1), 40);
        warm(&url(2), 40);
        assert_eq!(cdn.paths.lock().unwrap().len(), 1, "{status}: asked again");
        assert_eq!(limiter.stopped(), Some(stop), "{status}");
        assert_eq!(limiter.retry_after(), Duration::from_secs(120), "{status}");
        assert_eq!(store.age(&covers::art_key(&url(1))), None, "{status}");
    }
}

#[test]
fn art_outside_the_apple_cdn_is_never_requested() {
    let (store, limiter) = (art_store("host"), limiter(10));
    let mut warm = fetcher(store, limiter.clone(), None);
    let asked = AtomicUsize::new(0);
    for url in [
        "http://127.0.0.1:9/a.jpg",
        "https://example.com/a.jpg",
        "https://a@x.mzstatic.com/a.jpg",
    ] {
        warm(url, 40);
        asked.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!((asked.load(Ordering::SeqCst), limiter.requests()), (3, 0));
}

#[test]
fn an_art_transport_retry_spends_the_budget_too() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = accepted.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            counted.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let (store, limiter) = (art_store("retry"), limiter(1));
    let mut warm = fetcher(store, limiter.clone(), Some(base.clone()));
    warm(&format!("{base}/art/1/40x40bb.jpg"), 40);
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the retry went out unpaid"
    );
    assert_eq!(limiter.requests(), 1);
}

fn stopped(stop: Stop) -> Report {
    Report {
        stop,
        ..Report::default()
    }
}

fn now_ms() -> u64 {
    super::millis(std::time::SystemTime::now())
}

#[test]
fn a_refusal_pauses_every_run_until_the_app_has_started_again() {
    let root = scratch("refused");
    record(&root, &stopped(Stop::Refused), Duration::ZERO, now_ms());
    assert_eq!(paused(&root, now_ms()), Some(Stop::Refused));
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(root.join("scale"), "1").unwrap();
    assert_eq!(paused(&root, now_ms()), None, "the app has run since");
    record(&root, &stopped(Stop::Refused), Duration::ZERO, now_ms());
    assert_eq!(paused(&root, now_ms()), Some(Stop::Refused));
    record(&root, &stopped(Stop::Complete), Duration::ZERO, now_ms());
    assert_eq!(paused(&root, now_ms()), None);
}

#[test]
fn a_429_or_5xx_backs_off_from_5_minutes_to_an_hour_and_a_clean_run_clears_it() {
    let root = scratch("backoff");
    let minute = 60_000;
    let first = stopped(Stop::RateLimited);
    record(&root, &first, Duration::from_secs(1), 0);
    assert_eq!(paused(&root, 5 * minute - 1), Some(Stop::Backoff));
    assert_eq!(paused(&root, 5 * minute), None);
    let again = stopped(Stop::ServerError);
    record(&root, &again, Duration::ZERO, 10 * minute);
    assert_eq!(paused(&root, 19 * minute), Some(Stop::Backoff));
    assert_eq!(paused(&root, 20 * minute), None, "5 then 10 minutes");
    for _ in 0..8 {
        record(&root, &again, Duration::ZERO, 0);
    }
    assert_eq!(paused(&root, 59 * minute), Some(Stop::Backoff));
    assert_eq!(paused(&root, 60 * minute), None, "capped at an hour");
    record(&root, &first, Duration::from_secs(7200), 0);
    assert_eq!(
        paused(&root, 119 * minute),
        Some(Stop::Backoff),
        "Retry-After wins"
    );
    record(&root, &stopped(Stop::AppRunning), Duration::ZERO, 0);
    assert_eq!(
        paused(&root, 1),
        Some(Stop::Backoff),
        "an unrelated stop clears nothing"
    );
    record(&root, &stopped(Stop::Budget), Duration::ZERO, 0);
    assert_eq!(paused(&root, 1), None);
}

#[test]
fn a_paused_run_makes_no_request_and_a_signed_in_user_is_not_signed_out() {
    for (state, stop) in [
        ("refused 1 0", Stop::Refused),
        ("backoff 99999999999999 1", Stop::Backoff),
    ] {
        let dir = scratch(&format!("paused-{}", stop.name()));
        let user = mock_user(&dir);
        user.save("token", || true).unwrap();
        let root = dir.join("cache");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("warm.state"), state).unwrap();
        let slot = Slot::at(dir.join("slot"), "bramley");
        assert_eq!(start(user.clone(), slot, root).unwrap(), stopped(stop));
        assert_eq!(user.load(), Ok(Some("token".into())));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_locked_or_unanswered_keyring_counts_as_locked_and_only_no_service_at_all_does_not() {
    use crate::credentials::locked_by;
    let panics = || panic!("no secret service, so no collection to ask");
    assert!(!locked_by(Some(false), panics));
    assert!(locked_by(None, || Some(false)));
    assert!(locked_by(Some(true), || None));
    assert!(locked_by(Some(true), || Some(true)));
    assert!(!locked_by(Some(true), || Some(false)));
}
