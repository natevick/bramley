use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

use super::*;
use crate::store::Store;
use crate::warm::{Limiter, Report, Sizes, Stop, run};

const SIZES: Sizes = Sizes {
    cell: 140,
    row: 40,
    home: 96,
};
const SHELVES: [&str; 4] = ["albums", "artists", "songs", "playlists"];

fn scratch(name: &str) -> PathBuf {
    let tag = format!("warm-{}-{name}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let dir = dir.join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn rows(kind: &str, prefix: &str, count: usize) -> String {
    let row = |n: usize| {
        let art = format!(r#""artwork":{{"url":"https://art.test/{prefix}{n}/{{w}}x{{h}}.jpg"}}"#);
        format!(r#"{{"id":"{prefix}.{n}","type":"{kind}","attributes":{{"name":"n",{art}}}}}"#)
    };
    let all: Vec<String> = (1..=count).map(row).collect();
    format!(
        r#"{{"data":[{}],"meta":{{"total":{count}}}}}"#,
        all.join(",")
    )
}

fn library(albums: usize, playlists: usize) -> Vec<(String, Vec<Response>)> {
    let page = |body: String| vec![ok(&body)];
    let mut routes = vec![
        (
            "/v1/me/library/albums".into(),
            page(rows("library-albums", "l", albums)),
        ),
        (
            "/v1/me/library/artists".into(),
            page(rows("library-artists", "r", 1)),
        ),
        (
            "/v1/me/library/songs".into(),
            page(rows("library-songs", "i", 2)),
        ),
        (
            "/v1/me/library/playlists".into(),
            page(rows("library-playlists", "p", playlists)),
        ),
    ];
    for (shelf, id, count) in [("albums", "l", albums), ("playlists", "p", playlists)] {
        for n in 1..=count {
            let path = format!("/v1/me/library/{shelf}/{id}.{n}/tracks");
            routes.push((path, page(rows("library-songs", "t", 1))));
        }
    }
    routes
}

fn library_server(albums: usize, playlists: usize) -> Server {
    let routes = library(albums, playlists);
    serve(
        routes
            .iter()
            .map(|(p, r)| (p.as_str(), r.clone()))
            .collect(),
    )
}

struct Warmed {
    root: PathBuf,
    store: Arc<Store>,
    limiter: Arc<Limiter>,
    client: Client,
}

fn warmed(server: &Server, name: &str, (budget, pace): (usize, Duration)) -> Warmed {
    let root = scratch(name);
    let store = Arc::new(Store::open(&root, "api", 1 << 20).unwrap());
    let limiter = Arc::new(Limiter::new(budget, pace, || false));
    let client = raw(&server.base, Some(USER))
        .caching(store.clone())
        .warming(limiter.clone());
    Warmed {
        root,
        store,
        limiter,
        client,
    }
}

fn plain(server: &Server, warmed: &Warmed) -> Client {
    raw(&server.base, Some(USER)).caching(warmed.store.clone())
}

fn nothing(_: &str, _: u32) {}

fn go(w: &Warmed) -> Report {
    run(&w.client, &w.limiter, None, &mut nothing)
}

fn rerun(server: &Server, w: &Warmed) -> Report {
    let limiter = Arc::new(Limiter::new(60, Duration::ZERO, || false));
    let client = raw(&server.base, Some(USER))
        .caching(w.store.clone())
        .warming(limiter.clone());
    run(&client, &limiter, None, &mut nothing)
}

fn backdate(w: &Warmed, seconds: u64) {
    let then = SystemTime::now() - Duration::from_secs(seconds);
    for entry in std::fs::read_dir(w.root.join("api")).unwrap() {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(entry.unwrap().path())
            .unwrap();
        file.set_modified(then).unwrap();
    }
}

fn serving(routes: &[(String, Vec<Response>)]) -> Server {
    serve(
        routes
            .iter()
            .map(|(p, r)| (p.as_str(), r.clone()))
            .collect(),
    )
}

fn paths(server: &Server) -> Vec<String> {
    let found = server.targets();
    let path = |t: &String| t.split('?').next().unwrap().to_owned();
    found.iter().map(path).collect()
}

fn snapshot(store: &Store, server: &Server) -> Vec<(String, Option<Vec<u8>>)> {
    let paths = server.targets();
    let keys = paths
        .iter()
        .map(|t| t.split('?').next().unwrap().to_owned());
    keys.map(|p| {
        (
            p.clone(),
            store.get(&["api", &digest(&[USER]), &format!("{p}?limit=100")]),
        )
    })
    .collect()
}

fn digest(parts: &[&str]) -> String {
    crate::store::digest(parts)
}

fn fast() -> (usize, Duration) {
    (60, Duration::ZERO)
}

#[test]
fn the_four_library_lists_come_first_then_track_lists_and_nothing_else() {
    let server = library_server(2, 1);
    let w = warmed(&server, "order", fast());
    let report = go(&w);
    let paths: Vec<String> = server
        .targets()
        .iter()
        .map(|t| t.split('?').next().unwrap().to_owned())
        .collect();
    let lists: Vec<String> = SHELVES
        .iter()
        .map(|s| format!("/v1/me/library/{s}"))
        .collect();
    assert_eq!(paths[..4], lists[..]);
    let tracks = ["albums/l.1", "albums/l.2", "playlists/p.1"];
    for (path, tail) in paths[4..].iter().zip(tracks) {
        assert_eq!(path, &format!("/v1/me/library/{tail}/tracks"));
    }
    assert_eq!(
        (paths.len(), report.requests, report.stop),
        (7, 7, Stop::Complete)
    );
}

#[test]
fn the_request_budget_is_enforced_by_the_client_and_by_the_run() {
    let server = library_server(3, 0);
    let w = warmed(&server, "budget", (3, Duration::ZERO));
    let api = w.client.session().unwrap();
    for shelf in [Shelf::Albums, Shelf::Artists, Shelf::Songs] {
        api.library(shelf).unwrap();
    }
    assert!(matches!(
        api.library(Shelf::Playlists),
        Err(Error::Refused(_))
    ));
    assert_eq!(server.targets().len(), 3);

    let server = library_server(3, 0);
    let w = warmed(&server, "budget-run", (7, Duration::ZERO));
    let report = go(&w);
    assert_eq!(server.targets().len(), 6, "{report:?}");
    assert_eq!((report.requests, report.stop), (6, Stop::Budget));
}

#[test]
fn requests_are_paced() {
    let server = library_server(1, 0);
    let pace = Duration::from_millis(150);
    let w = warmed(&server, "pace", (60, pace));
    let started = Instant::now();
    let report = go(&w);
    assert_eq!(report.requests, 5);
    assert!(started.elapsed() >= 4 * pace, "{:?}", started.elapsed());
}

#[test]
fn a_429_or_5xx_stops_the_run_without_a_retry_or_another_request() {
    for status in [429, 500, 503] {
        let mut routes = library(2, 1);
        let refused = (status, "Retry-After: 0\r\n".to_owned(), Vec::new());
        routes[2].1 = vec![refused];
        let server = serve(
            routes
                .iter()
                .map(|(p, r)| (p.as_str(), r.clone()))
                .collect(),
        );
        let w = warmed(&server, &format!("stop-{status}"), fast());
        let report = go(&w);
        let paths = server.targets();
        assert_eq!(paths.len(), 3, "{status}: {paths:?}");
        assert!(paths[2].starts_with("/v1/me/library/songs"));
        let expected = if status == 429 {
            Stop::RateLimited
        } else {
            Stop::ServerError
        };
        assert_eq!(report.stop, expected, "{status}");
    }
}

#[test]
fn a_401_or_403_stops_and_leaves_the_cache_as_it_was() {
    for status in [401, 403] {
        let server = library_server(2, 1);
        let w = warmed(&server, &format!("auth-{status}"), fast());
        assert_eq!(go(&w).stop, Stop::Complete);
        let before = snapshot(&w.store, &server);
        assert!(before.iter().all(|(_, body)| body.is_some()));
        backdate(&w, 3600);

        let mut routes = library(2, 1);
        routes[0].1 = vec![(status, String::new(), Vec::new())];
        let refusing = serve(
            routes
                .iter()
                .map(|(p, r)| (p.as_str(), r.clone()))
                .collect(),
        );
        let limiter = Arc::new(Limiter::new(60, Duration::ZERO, || false));
        let client = raw(&refusing.base, Some(USER))
            .caching(w.store.clone())
            .warming(limiter.clone());
        let report = run(&client, &limiter, None, &mut nothing);
        assert_eq!(
            (report.stop, refusing.targets().len()),
            (Stop::Refused, 1),
            "{status}"
        );
        assert_eq!(snapshot(&w.store, &server), before, "{status}");
    }
}

#[test]
fn listening_history_is_never_requested() {
    let server = library_server(2, 1);
    let w = warmed(&server, "history", fast());
    let sizes = Some(SIZES);
    run(&w.client, &w.limiter, sizes, &mut |_, _| {});
    let targets = server.targets();
    for target in &targets {
        for banned in [
            "recent",
            "history",
            "music-summaries",
            "recently-added",
            "heavy-rotation",
        ] {
            assert!(!target.contains(banned), "{target}");
        }
        assert!(target.starts_with("/v1/me/library/"), "{target}");
    }
    assert!(!targets.is_empty());
}

#[test]
fn a_run_stops_once_the_app_starts() {
    let server = library_server(3, 0);
    let store = Arc::new(Store::open(&scratch("app"), "api", 1 << 20).unwrap());
    let started = Arc::new(AtomicBool::new(false));
    let flag = started.clone();
    let limiter = Arc::new(Limiter::new(60, Duration::ZERO, move || {
        flag.load(Ordering::SeqCst)
    }));
    let client = raw(&server.base, Some(USER))
        .caching(store)
        .warming(limiter.clone());
    let report = run(&client, &limiter, Some(SIZES), &mut |_, _| {
        started.store(true, Ordering::SeqCst);
    });
    assert_eq!(report.stop, Stop::AppRunning);
    assert_eq!(
        server.targets().len(),
        5,
        "four lists and one track list, then it stopped"
    );
}

#[test]
fn an_app_that_starts_mid_request_gets_no_write_from_warm() {
    let server = library_server(1, 0);
    let store = Arc::new(Store::open(&scratch("late"), "api", 1 << 20).unwrap());
    let polls = AtomicUsize::new(0);
    let limiter = Arc::new(Limiter::new(60, Duration::ZERO, move || {
        polls.fetch_add(1, Ordering::SeqCst) >= 1
    }));
    let client = raw(&server.base, Some(USER))
        .caching(store.clone())
        .warming(limiter.clone());
    client.session().unwrap().library(Shelf::Albums).unwrap();
    assert_eq!(server.targets().len(), 1);
    let app = raw(&server.base, Some(USER)).caching(store);
    assert_eq!(app.stale(&Call::Library(Shelf::Albums)), None);
}

#[test]
fn fresh_entries_make_no_request_and_the_oldest_track_lists_are_refreshed_first() {
    let server = library_server(3, 0);
    let w = warmed(&server, "fresh", fast());
    go(&w);
    let tracks = |n: usize| format!("/v1/me/library/albums/l.{n}/tracks");
    let before = server.targets().len();
    let steady = rerun(&server, &w);
    assert_eq!(
        (
            server.targets().len() - before,
            steady.requests,
            steady.stop
        ),
        (0, 0, Stop::Complete)
    );

    let age = |n: usize, seconds: u64| {
        let key = ["api", &digest(&[USER]), &format!("{}?limit=100", tracks(n))];
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(w.store.path(&key))
            .unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    };
    age(2, 7 * 3600);
    age(3, 9 * 3600);
    let before = server.targets().len();
    rerun(&server, &w);
    assert_eq!(paths(&server)[before..], [tracks(3), tracks(2)]);
}

#[test]
fn list_pages_are_refreshed_only_after_30_minutes() {
    let server = library_server(2, 1);
    let w = warmed(&server, "lists", fast());
    go(&w);
    backdate(&w, 25 * 60);
    assert_eq!(rerun(&server, &w).requests, 0);
    backdate(&w, 31 * 60);
    let lists = paths(&server).len();
    let report = rerun(&server, &w);
    let refreshed = &paths(&server)[lists..];
    assert_eq!(report.requests, 4, "{refreshed:?}");
    assert!(
        refreshed[..4]
            .iter()
            .all(|p| SHELVES.iter().any(|s| p.ends_with(s)))
    );
}

#[test]
fn one_broken_item_is_skipped_and_the_rest_are_still_warmed() {
    let mut routes = library(3, 1);
    let gone = routes
        .iter_mut()
        .find(|r| r.0.ends_with("l.1/tracks"))
        .unwrap();
    gone.1 = vec![(404, String::new(), Vec::new())];
    let server = serving(&routes);
    let w = warmed(&server, "broken", fast());
    let report = go(&w);
    let asked = paths(&server);
    for tail in ["albums/l.2", "albums/l.3", "playlists/p.1"] {
        let path = format!("/v1/me/library/{tail}/tracks");
        assert!(asked.contains(&path), "{path} was not asked: {asked:?}");
    }
    assert_eq!((report.skipped, report.stop), (1, Stop::Complete));
    let again = rerun(&server, &w);
    assert_eq!(
        (again.requests, again.skipped),
        (1, 1),
        "only the broken one is asked again"
    );
}

#[test]
fn an_empty_playlist_is_cached_as_empty_and_not_asked_for_every_run() {
    let mut routes = library(1, 1);
    let empty = r#"{"errors":[{"detail":"No related resources found for tracks"}]}"#;
    let gone = routes
        .iter_mut()
        .find(|r| r.0.ends_with("p.1/tracks"))
        .unwrap();
    gone.1 = vec![(404, String::new(), empty.as_bytes().to_vec())];
    let server = serving(&routes);
    let w = warmed(&server, "empty", fast());
    assert_eq!(go(&w).stop, Stop::Complete);
    backdate(&w, 31 * 60);
    rerun(&server, &w);
    let asked = paths(&server);
    let tracks = asked.iter().filter(|p| p.ends_with("p.1/tracks")).count();
    assert_eq!(tracks, 1);
    let page = plain(&server, &w).stale(&Call::PlaylistTracks("p.1".into()));
    assert!(matches!(page, Some(Reply::Page(page)) if page.items.is_empty()));
}

#[test]
fn a_transport_retry_spends_the_budget_too() {
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
    let limiter = Arc::new(Limiter::new(1, Duration::ZERO, || false));
    let client = raw(&base, Some(USER)).warming(limiter.clone());
    assert!(client.session().unwrap().library(Shelf::Albums).is_err());
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the retry went out unpaid"
    );
    assert_eq!(limiter.requests(), 1);
}

#[test]
fn what_warm_writes_is_what_the_app_reads() {
    let server = library_server(2, 1);
    let w = warmed(&server, "keys", fast());
    go(&w);
    let app = plain(&server, &w);
    for call in [
        Call::Library(Shelf::Albums),
        Call::Library(Shelf::Playlists),
        Call::AlbumTracks("l.2".into()),
        Call::PlaylistTracks("p.1".into()),
    ] {
        assert!(app.stale(&call).is_some(), "{call:?}");
    }
}

#[test]
fn covers_are_asked_for_at_the_surface_sizes_each_once_and_not_without_a_scale() {
    let server = library_server(2, 1);
    let w = warmed(&server, "art", fast());
    let mut asked: Vec<(String, u32)> = Vec::new();
    let report = run(&w.client, &w.limiter, Some(SIZES), &mut |url, px| {
        asked.push((url.to_owned(), px));
    });
    let url = |id: &str, px: u32| format!("https://art.test/{id}/{px}x{px}.jpg");
    let expected = [
        (url("t1", 40), 40),
        (url("t1", 40), 40),
        (url("t1", 40), 40),
        (url("l1", 140), 140),
        (url("l2", 140), 140),
        (url("l1", 96), 96),
        (url("l2", 96), 96),
        (url("p1", 40), 40),
        (url("i1", 40), 40),
        (url("i2", 40), 40),
    ];
    let mut unique = Vec::new();
    for pair in expected {
        if !unique.contains(&pair) {
            unique.push(pair);
        }
    }
    assert_eq!(asked, unique);
    assert_eq!(report.stop, Stop::Complete);

    let server = library_server(2, 1);
    let w = warmed(&server, "no-scale", fast());
    let report = run(&w.client, &w.limiter, None, &mut |_, _| {
        panic!("no scale, no art")
    });
    assert_eq!(report.stop, Stop::Complete);
}

fn paged(routes: &mut [(String, Vec<Response>)], first: &str, second: &str) {
    let more = r#""next":"/v1/me/library/albums?offset=100","meta""#;
    let first = first.replace(r#""meta""#, more);
    routes[0].1 = vec![ok(&first), ok(second), ok(&first)];
}

#[test]
fn later_album_pages_are_followed_and_refreshed_with_the_first_page_after_30_minutes() {
    let mut routes = library(2, 0);
    let (first, second) = (
        rows("library-albums", "l", 1),
        rows("library-albums", "l", 2),
    );
    paged(&mut routes, &first, &second);
    let server = serve(
        routes
            .iter()
            .map(|(p, r)| (p.as_str(), r.clone()))
            .collect(),
    );
    let w = warmed(&server, "later", fast());
    let on_albums = |server: &Server| {
        let targets = server.targets();
        targets
            .iter()
            .filter(|t| t.starts_with("/v1/me/library/albums?"))
            .count()
    };
    assert_eq!(go(&w).stop, Stop::Complete);
    assert_eq!(on_albums(&server), 2);
    assert!(
        plain(&server, &w)
            .stale(&Call::Next("/v1/me/library/albums?offset=100".into()))
            .is_some()
    );

    rerun(&server, &w);
    assert_eq!(on_albums(&server), 2, "fresh pages are read, not requested");
    backdate(&w, 31 * 60);
    rerun(&server, &w);
    assert_eq!(on_albums(&server), 4, "both pages were past 30 minutes");
}
