use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::client::allowed;
use crate::{Call, Client, Error, Job, Kind, Reply, Shelf, Tokens, worker};

const DEVELOPER: &str = "dev-token-fixture-0123456789";
const USER: &str = "user-token-fixture-9876543210";

type Response = (u16, String, Vec<u8>);

struct Hit {
    target: String,
    headers: HashMap<String, String>,
}

struct Server {
    base: String,
    hits: Arc<Mutex<Vec<Hit>>>,
}

impl Server {
    fn targets(&self) -> Vec<String> {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .map(|h| h.target.clone())
            .collect()
    }

    fn header(&self, n: usize, name: &str) -> Option<String> {
        self.hits.lock().unwrap()[n].headers.get(name).cloned()
    }
}

fn ok(fixture: &str) -> Response {
    (200, String::new(), fixture.as_bytes().to_vec())
}

fn serve(routes: Vec<(&str, Vec<Response>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Mutex::new(Vec::new()));
    let seen = hits.clone();
    let mut routes: HashMap<String, Vec<Response>> =
        routes.into_iter().map(|(p, r)| (p.to_owned(), r)).collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let target = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let Some((k, v)) = line.split_once(':') else {
                    break;
                };
                headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
            }
            let path = target.split('?').next().unwrap().to_owned();
            seen.lock().unwrap().push(Hit { target, headers });
            let queue = routes.get_mut(&path);
            let (status, extra, body) = match queue {
                Some(q) if q.len() > 1 => q.remove(0),
                Some(q) => q[0].clone(),
                None => (404, String::new(), Vec::new()),
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body));
        }
    });
    Server { base, hits }
}

fn client(server: &Server, user: Option<&str>) -> Client {
    let user = user.map(str::to_owned);
    Client::at(&server.base, move || {
        Ok(Tokens {
            developer: DEVELOPER.into(),
            user: user.clone(),
        })
    })
}

const STOREFRONT: &str = include_str!("fixtures/storefront.json");
const SEARCH: &str = include_str!("fixtures/search.json");

fn searchable(search: Vec<Response>) -> Server {
    serve(vec![
        ("/v1/me/storefront", vec![ok(STOREFRONT)]),
        ("/v1/catalog/us/search", search),
    ])
}

#[test]
fn search_reads_each_section_ignores_unknown_fields_and_sends_the_user_token_only_to_me() {
    let server = searchable(vec![ok(SEARCH)]);
    let found = client(&server, Some(USER))
        .search("bohemian rhapsody")
        .unwrap();
    let song = &found.songs.items[0];
    assert_eq!(
        (
            song.id.as_str(),
            song.kind,
            song.name.as_deref(),
            song.artist.as_deref()
        ),
        ("1000001", Kind::Songs, Some("Song A"), Some("Band A"))
    );
    assert_eq!(
        (song.album.as_deref(), song.duration_ms),
        (Some("Album A"), Some(354320))
    );
    assert_eq!(
        crate::art_url(song.artwork.as_deref().unwrap(), 96).as_deref(),
        Some("https://is1.mzstatic.com/x/96x96bb.jpg")
    );
    assert_eq!(found.songs.items[1].artwork, None);
    assert_eq!(found.albums.items[0].track_count, Some(12));
    assert_eq!(found.artists.items[0].kind, Kind::Artists);
    assert_eq!(found.playlists.items[0].id, "pl.0123456789abcdef");
    let targets = server.targets();
    assert_eq!(targets[0], "/v1/me/storefront");
    assert_eq!(
        targets[1],
        "/v1/catalog/us/search?term=bohemian%20rhapsody&types=songs%2Calbums%2Cartists%2Cplaylists&limit=10"
    );
    assert_eq!(server.header(0, "music-user-token").as_deref(), Some(USER));
    assert_eq!(server.header(1, "music-user-token"), None);
    for n in 0..2 {
        let bearer = format!("Bearer {DEVELOPER}");
        assert_eq!(server.header(n, "authorization"), Some(bearer));
        assert!(
            server
                .header(n, "user-agent")
                .unwrap()
                .starts_with("Bramley/")
        );
        assert_eq!(server.header(n, "origin"), None);
    }
}

#[test]
fn library_pages_follow_next_with_the_library_limit_and_report_the_total() {
    let server = serve(vec![(
        "/v1/me/library/playlists",
        vec![
            ok(include_str!("fixtures/library-playlists-1.json")),
            ok(include_str!("fixtures/library-playlists-2.json")),
        ],
    )]);
    let api = client(&server, Some(USER));
    let first = api.library(Shelf::Playlists).unwrap();
    assert_eq!((first.items.len(), first.total), (2, Some(3)));
    let all = api.all(first).unwrap();
    let ids: Vec<_> = all.iter().map(|i| (i.id.as_str(), i.kind)).collect();
    assert_eq!(
        ids,
        [
            ("p.AAAAAAAAAAAAAAA", Kind::LibraryPlaylists),
            ("p.BBBBBBBBBBBBBBB", Kind::LibraryPlaylists),
            ("p.CCCCCCCCCCCCCCC", Kind::LibraryPlaylists),
            ("i.DDDDDDDDDDDDDDD", Kind::Other),
        ]
    );
    assert_eq!(
        server.targets(),
        [
            "/v1/me/library/playlists?limit=100",
            "/v1/me/library/playlists?offset=2&limit=100"
        ]
    );
}

#[test]
fn an_empty_library_playlist_is_an_empty_page_not_an_error() {
    let none =
        r#"{"errors": [{"status": "404", "detail": "No related resources found for tracks"}]}"#;
    let path = "/v1/me/library/playlists/p.EMPTY/tracks";
    let server = serve(vec![(path, vec![(404, String::new(), none.into())])]);
    let page = client(&server, Some(USER))
        .playlist_tracks("p.EMPTY")
        .unwrap();
    assert_eq!(
        (page.items.len(), page.total, page.next),
        (0, Some(0), None)
    );
}

#[test]
fn an_album_brings_its_first_tracks_and_pages_the_rest() {
    let server = serve(vec![
        ("/v1/me/storefront", vec![ok(STOREFRONT)]),
        (
            "/v1/catalog/us/albums/2000001",
            vec![ok(include_str!("fixtures/album.json"))],
        ),
        (
            "/v1/catalog/us/albums/2000001/tracks",
            vec![ok(include_str!("fixtures/album-tracks-2.json"))],
        ),
    ]);
    let api = client(&server, Some(USER));
    let album = api.album("2000001").unwrap();
    assert_eq!(
        (album.item.name.as_deref(), album.item.track_count),
        (Some("Album A"), Some(3))
    );
    let tracks = api.all(album.tracks).unwrap();
    let ids: Vec<_> = tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["1000001", "1000003", "1000004"]);
    assert_eq!(
        server.targets()[2],
        "/v1/catalog/us/albums/2000001/tracks?offset=2"
    );
}

#[test]
fn statuses_map_to_typed_errors_without_either_token_in_the_text() {
    let echo = format!(r#"{{"errors": [{{"detail": "bad {DEVELOPER} for {USER}"}}]}}"#);
    let cases = [
        (401, Error::DeveloperToken),
        (403, Error::SignInNeeded),
        (
            500,
            Error::Http(500, "bad <redacted> for <redacted>".into()),
        ),
        (
            503,
            Error::Http(503, "bad <redacted> for <redacted>".into()),
        ),
    ];
    for (status, want) in cases {
        let server = serve(vec![(
            "/v1/me/library/songs",
            vec![(status, String::new(), echo.clone().into_bytes())],
        )]);
        let got = client(&server, Some(USER))
            .library(Shelf::Songs)
            .unwrap_err();
        assert_eq!(got, want, "{status}");
        let text = got.to_string();
        assert!(!text.contains(DEVELOPER) && !text.contains(USER), "{text}");
    }
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let gone = Client::at(&base, || {
        Ok(Tokens {
            developer: DEVELOPER.into(),
            user: Some(USER.into()),
        })
    });
    let text = gone.library(Shelf::Songs).unwrap_err().to_string();
    assert!(
        !text.is_empty() && !text.contains(DEVELOPER) && !text.contains(USER),
        "{text}"
    );
}

#[test]
fn a_429_waits_out_a_short_retry_after_and_refuses_a_long_one_or_an_endless_one() {
    let wait = |after: &str| (429, format!("Retry-After: {after}\r\n"), Vec::new());
    let server = searchable(vec![wait("1"), ok(SEARCH)]);
    let started = Instant::now();
    assert!(client(&server, Some(USER)).search("a").is_ok());
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(server.targets().len(), 3);

    let server = searchable(vec![wait("120")]);
    let started = Instant::now();
    assert_eq!(
        client(&server, Some(USER)).search("a"),
        Err(Error::RateLimited(120))
    );
    assert!(started.elapsed() < Duration::from_secs(1));

    let server = searchable(vec![wait("0")]);
    assert_eq!(
        client(&server, Some(USER)).search("a"),
        Err(Error::RateLimited(0))
    );
    assert_eq!(server.targets().len(), 1 + 3);
}

#[test]
fn only_documented_api_paths_are_requested() {
    for good in [
        "/v1/me/storefront",
        "/v1/me/library/songs",
        "/v1/me/library/playlists/p.AAA/tracks",
        "/v1/catalog/us/search",
        "/v1/catalog/us/albums/2000001",
        "/v1/catalog/gb/playlists/pl.u-abc_d/tracks",
    ] {
        assert!(allowed(good), "{good}");
    }
    let server = serve(vec![]);
    let api = client(&server, Some(USER));
    for bad in [
        "https://evil.example/v1/me/library/songs",
        "//evil.example/v1/me/library/songs",
        "/v1/me/library/../storefront",
        "/v1/me/library/songs/../../x",
        "/v1/catalog/us/charts",
        "/v1/me/recent/played",
        "/v1/catalog/us/albums/1/tracks/2",
        "v1/me/storefront",
        "@evil.example/v1/me/storefront",
    ] {
        assert!(matches!(api.next(bad), Err(Error::Refused(_))), "{bad}");
    }
    for bad in ["1/tracks", "..", "", "1?x=y"] {
        assert!(
            matches!(api.playlist_tracks(bad), Err(Error::Refused(_)))
                && matches!(api.album(bad), Err(Error::Refused(_))),
            "{bad:?}"
        );
    }
    assert!(server.targets().is_empty());
}

#[test]
fn a_redirect_is_refused_and_never_followed() {
    let elsewhere = serve(vec![("/v1/me/storefront", vec![ok(STOREFRONT)])]);
    let location = format!("Location: {}/v1/me/storefront\r\n", elsewhere.base);
    let server = serve(vec![(
        "/v1/me/storefront",
        vec![(302, location, Vec::new())],
    )]);
    let got = client(&server, Some(USER)).storefront();
    assert!(
        matches!(got, Err(Error::Refused(ref why)) if why.contains("302")),
        "{got:?}"
    );
    assert!(elsewhere.targets().is_empty());
}

#[test]
fn an_oversized_body_is_refused() {
    let big = format!(
        r#"{{"data": [{{"id": "us", "pad": "{}"}}]}}"#,
        "x".repeat(3 << 20)
    );
    let server = serve(vec![("/v1/me/storefront", vec![ok(&big)])]);
    assert_eq!(
        client(&server, Some(USER)).storefront(),
        Err(Error::TooLarge)
    );
}

#[test]
fn signed_out_library_calls_ask_for_sign_in_without_a_request() {
    let server = serve(vec![]);
    let got = client(&server, None).library(Shelf::Albums);
    assert_eq!(got, Err(Error::SignInNeeded));
    assert!(server.targets().is_empty());
}

#[test]
fn the_worker_answers_each_job_off_the_calling_thread() {
    let server = searchable(vec![ok(SEARCH)]);
    let jobs = worker(client(&server, Some(USER)));
    let (tx, rx) = channel();
    let caller = std::thread::current().id();
    for call in [Call::Search("a".into()), Call::Album("..".into())] {
        let tx = tx.clone();
        let job = Job::new(call, move |r| {
            tx.send((r, std::thread::current().id())).unwrap()
        });
        jobs.send(job).unwrap();
    }
    let (found, ran_on) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(found, Ok(Reply::Search(ref s)) if s.songs.items[0].id == "1000001"));
    assert_ne!(ran_on, caller);
    let (refused, _) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
}

#[cfg(bramley_tokens)]
#[test]
fn live_search_and_the_largest_library_playlist() {
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".config/bramley");
    let read =
        move |name: &str| std::fs::read_to_string(dir.join(name)).map(|t| t.trim().to_owned());
    let api = Client::new(move || {
        let developer = read("developer-token").map_err(|e| e.to_string())?;
        Ok(Tokens {
            developer,
            user: read("music-user-token").ok(),
        })
    });
    let found = api.search("bohemian rhapsody").unwrap();
    assert!(found.songs.items.iter().any(|s| s.id == "6781024437"));
    let playlists = api.all(api.library(Shelf::Playlists).unwrap()).unwrap();
    let mut largest = (0, String::new());
    for playlist in &playlists {
        let total = api
            .playlist_tracks(&playlist.id)
            .unwrap()
            .total
            .unwrap_or(0);
        largest = largest.max((total, playlist.id.clone()));
    }
    let tracks = api.all(api.playlist_tracks(&largest.1).unwrap()).unwrap();
    assert_eq!(tracks.len() as u32, largest.0);
    let albums = api.library(Shelf::Albums).unwrap().total;
    let songs = api.library(Shelf::Songs).unwrap().total;
    eprintln!(
        "live: {} playlists; playlist A ({} tracks, {} listed); albums {albums:?}; songs {songs:?}",
        playlists.len(),
        largest.0,
        tracks.len()
    );
}
