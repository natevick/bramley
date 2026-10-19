use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bramley_api::store::Store;
use bramley_api::warm::{Art, Limiter, Report, Stop};
use fastframe_instance::{Claim, Slot};

use super::{fetcher, lock, recorded_scale, running, sizes, start};
use crate::covers::{self, Tally};
use crate::tests::mock_user;

fn scratch(name: &str) -> std::path::PathBuf {
    let tag = format!("warm-{}-{name}", std::process::id());
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let dir = dir.join(tag);
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
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
    assert!(limiter.api().is_ok());
    let starting = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let guard = slot.claim("show", |_| Some("ok".into()));
        std::thread::sleep(Duration::from_secs(1));
        drop(guard);
    });
    let begun = Instant::now();
    while limiter.api().is_ok() {
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
    let report = start(mock_user(&dir), Slot::at(dir.join("slot"), "bramley")).unwrap();
    assert_eq!(
        report,
        Report {
            stop: Stop::NoToken,
            ..Report::default()
        }
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
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
    assert!(matches!(warm(&url, 40), Art::Cached));
    assert!(matches!(warm(&url, 40), Art::Skipped));
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
    let kinds: Vec<bool> = (0..4)
        .map(|n| matches!(warm(&url(n), 40), Art::Cached))
        .collect();
    assert_eq!(kinds, [true, true, false, false]);
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
        assert!(matches!(warm(&url(1), 40), Art::Skipped));
        assert!(matches!(warm(&url(2), 40), Art::Skipped));
        assert_eq!(cdn.paths.lock().unwrap().len(), 1, "{status}: asked again");
        assert_eq!(limiter.stopped(), Some(stop), "{status}");
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
        assert!(matches!(warm(url, 40), Art::Skipped));
        asked.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!((asked.load(Ordering::SeqCst), limiter.requests()), (3, 0));
}
