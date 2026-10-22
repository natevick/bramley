use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bramley_api::{Call, Error, Item, Kind, Page, Reply, Shelf};
use bramley_engine::{Queue, QueueItem, Snapshot};
use egui::{Event, PointerButton, Pos2, RawInput, Rect, vec2};

use super::{ART_CAP, DEADLINE, Library, List, QUEUE_CAP, RETRY, ROW_PT, show};
use crate::playback::Action;
use crate::ui::nav::{Edge, Nav, Route};

fn item(i: usize) -> Item {
    Item {
        id: format!("i.{i}"),
        kind: Kind::LibrarySongs,
        name: Some(format!("Secret Song {i}")),
        artist: Some("Secret Artist".into()),
        album: None,
        duration_ms: None,
        artwork: None,
        track_count: None,
        playable: true,
    }
}

fn page(range: std::ops::Range<usize>, next: Option<&str>) -> Reply {
    let items = range.map(item).collect();
    Reply::Page(Page {
        items,
        next: next.map(str::to_owned),
        total: Some(250),
    })
}

fn frame(
    ctx: &egui::Context,
    lib: &mut Library,
    route: &Route,
    height: f32,
    events: Vec<Event>,
) -> (Vec<Action>, Vec<Nav>) {
    scrolled(ctx, lib, (route, &mut None), height, events)
}

fn scrolled(
    ctx: &egui::Context,
    lib: &mut Library,
    (route, scroll): (&Route, &mut Option<Edge>),
    height: f32,
    events: Vec<Event>,
) -> (Vec<Action>, Vec<Nav>) {
    let (mut actions, mut navs) = (Vec::new(), Vec::new());
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, height));
    let input = RawInput {
        screen_rect: Some(screen),
        events,
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        show(ui, lib, (route, scroll), (&mut actions, &mut navs))
    });
    out.textures_delta.clear();
    lib.art.flush(ctx);
    (actions, navs)
}

fn answer(lib: &mut Library, ctx: &egui::Context, now: Instant, reply: Reply) -> Call {
    let mut jobs = lib.tick(now, true, || {});
    assert_eq!(jobs.len(), 1, "one page in flight");
    let job = jobs.remove(0);
    let call = job.call.clone();
    (job.done)(Ok(reply));
    lib.receive(ctx);
    call
}

fn refused_thrice(lib: &mut Library, ctx: &egui::Context, route: &Route) {
    for n in 1..=3 {
        let job = lib.tick(Instant::now() + RETRY * n, true, || {}).remove(0);
        (job.done)(Err(Error::SignInNeeded));
        lib.receive(ctx);
        frame(ctx, lib, route, 600.0, vec![]);
    }
}

#[test]
fn songs_page_in_as_the_list_nears_its_end_and_250_items_give_250_rows() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let first = lib.tick(now, true, || {}).remove(0);
    assert_eq!(first.call, Call::Library(Shelf::Songs));
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    (first.done)(Ok(page(0..100, Some("/v1/me/library/songs?offset=100"))));
    lib.receive(&ctx);
    assert!(
        lib.tick(now, true, || {}).is_empty(),
        "a want from before the page arrived is spent"
    );
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    assert!(
        lib.tick(now, true, || {}).is_empty(),
        "the top of the list asks for nothing more"
    );
    frame(&ctx, &mut lib, &Route::Songs, 6000.0, vec![]);
    let sent = lib.tick(now, true, || {});
    assert_eq!(
        sent.iter().map(|j| &j.call).collect::<Vec<_>>(),
        [&Call::Next("/v1/me/library/songs?offset=100".into())]
    );
    frame(&ctx, &mut lib, &Route::Songs, 6000.0, vec![]);
    assert!(
        lib.tick(now, true, || {}).is_empty(),
        "a page in flight is not asked for twice"
    );
    lib.lists[List::Songs as usize].wanted = true;
    let second = || page(100..200, Some("/v1/me/library/songs?offset=200"));
    answer(&mut lib, &ctx, now + DEADLINE, second());
    let timed_out = sent.into_iter().next().unwrap();
    (timed_out.done)(Ok(second()));
    lib.receive(&ctx);
    assert_eq!(
        lib.list(List::Songs).items.len(),
        200,
        "a timed-out reply that arrives late is dropped"
    );
    frame(&ctx, &mut lib, &Route::Songs, 12000.0, vec![]);
    answer(&mut lib, &ctx, now, page(200..250, None));
    frame(&ctx, &mut lib, &Route::Songs, 20000.0, vec![]);
    assert!(
        lib.tick(now, true, || {}).is_empty(),
        "the last page ends paging"
    );
    let status = lib.status();
    assert_eq!(
        (
            status["songs"]["loaded"].as_u64(),
            status["songs"]["total"].as_u64()
        ),
        (Some(250), Some(250))
    );
    assert_eq!(status["shownTo"], 250);
}

#[test]
fn only_the_visible_rows_are_built() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Songs as usize].items = (0..250).map(item).collect();
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let (built, _, shown_to) = lib.built;
    let visible = (600.0 / super::ROW_PT) as usize;
    assert!(
        built > 0 && built <= visible + 2,
        "{built} rows built for about {visible} visible"
    );
    assert_eq!(shown_to, built);
}

#[test]
fn the_art_cache_evicts_the_least_recently_used_texture_and_frees_it() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let allocated = || ctx.tex_manager().read().num_allocated();
    let before = allocated();
    for i in 0..ART_CAP + 10 {
        lib.art.insert(format!("i.{i}"), texture(&ctx, i));
        if i == ART_CAP - 1 {
            assert!(lib.art.get(&ctx, &item(0), ROW_PT).is_some());
        }
        lib.art.flush(&ctx);
    }
    assert_eq!(lib.art.textures.len(), ART_CAP);
    assert_eq!(allocated() - before, ART_CAP);
    assert!(
        lib.art.textures.contains_key("i.0"),
        "a recently used texture survives"
    );
    assert!(
        !lib.art.textures.contains_key("i.1"),
        "the least recently used goes first"
    );
}

fn texture(ctx: &egui::Context, i: usize) -> egui::TextureHandle {
    let image = egui::ColorImage::filled([2, 2], egui::Color32::RED);
    ctx.load_texture(format!("t{i}"), image, Default::default())
}

fn art_item(i: usize, base: &str) -> Item {
    Item {
        artwork: Some(format!("{base}/art/{i}/{{w}}x{{h}}bb.jpg")),
        ..item(i)
    }
}

#[test]
fn covers_on_screen_are_never_evicted_so_135_visible_settle_without_refetching() {
    let (ctx, base) = (egui::Context::default(), "http://127.0.0.1:9");
    let mut lib = Library::new(Some(base.into()));
    let items: Vec<_> = (0..135).map(|i| art_item(i, base)).collect();
    let mut wanted = Vec::new();
    for _ in 0..4 {
        for item in &items {
            lib.art.get(&ctx, item, ROW_PT);
        }
        wanted.push(lib.art.wanted.len());
        for (key, ..) in std::mem::take(&mut lib.art.wanted) {
            lib.art.insert(key, texture(&ctx, 0));
        }
        lib.art.flush(&ctx);
    }
    assert_eq!(
        wanted,
        [135, 0, 0, 0],
        "a visible cover was evicted and fetched again"
    );
    assert_eq!(lib.art.textures.len(), 135);
}

fn serve(ok: bool, delay: Duration) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let mut jpeg = Vec::new();
    let image = image::RgbImage::from_pixel(40, 40, image::Rgb([200, 50, 50]));
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image)
        .unwrap();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.read(&mut [0u8; 4096]);
            counted.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(delay);
            let (status, body): (&str, &[u8]) = if ok {
                ("200 OK", &jpeg)
            } else {
                ("404 Not Found", b"")
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(body));
        }
    });
    (base, hits)
}

fn songs_with_art(base: &str, n: usize) -> Library {
    let mut lib = Library::new(Some(base.into()));
    lib.lists[List::Songs as usize].items = (0..n).map(|i| art_item(i, base)).collect();
    lib
}

#[test]
fn a_failing_cover_is_not_fetched_again_until_its_retry_time() {
    let (base, hits) = serve(false, Duration::ZERO);
    let (ctx, mut lib) = (egui::Context::default(), songs_with_art(&base, 25));
    for _ in 0..100 {
        frame(&ctx, &mut lib, &Route::Songs, 2000.0, vec![]);
        lib.receive(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    let asked = hits.load(Ordering::SeqCst);
    assert!(
        (25..=30).contains(&asked),
        "{asked} requests for 25 failing covers"
    );
    assert_eq!(lib.art.failed.len(), 25);
    for until in lib.art.failed.values_mut() {
        *until = Instant::now();
    }
    frame(&ctx, &mut lib, &Route::Songs, 2000.0, vec![]);
    assert_eq!(
        lib.art.recent.len(),
        25,
        "after its retry time a failed cover is asked for again"
    );
}

#[test]
fn signing_out_drops_the_art_and_a_cover_landing_afterwards_is_never_uploaded() {
    let (base, _) = serve(true, Duration::from_millis(150));
    let (ctx, mut lib) = (egui::Context::default(), songs_with_art(&base, 25));
    let allocated = || ctx.tex_manager().read().num_allocated();
    let before = allocated();
    let until = Instant::now() + Duration::from_secs(10);
    while lib.art.textures.is_empty() && Instant::now() < until {
        frame(&ctx, &mut lib, &Route::Songs, 2000.0, vec![]);
        lib.receive(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !lib.art.textures.is_empty(),
        "a cover arrived before sign-out"
    );
    frame(&ctx, &mut lib, &Route::Songs, 2000.0, vec![]);
    lib.lists[List::Songs as usize].pages = 1;
    assert!(lib.tick(Instant::now(), false, || {}).is_empty());
    assert_eq!((lib.art.textures.len(), allocated()), (0, before));
    std::thread::sleep(Duration::from_millis(400));
    lib.receive(&ctx);
    assert!(lib.art.decoded > 0, "a cover landed after sign-out");
    assert_eq!((lib.art.textures.len(), allocated()), (0, before));
}

#[test]
fn an_error_waits_the_retry_and_a_403_stops_paging_until_signed_out() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let job = lib.tick(now, true, || {}).remove(0);
    (job.done)(Err(Error::Network("down".into())));
    lib.receive(&ctx);
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    assert!(
        lib.tick(Instant::now(), true, || {}).is_empty(),
        "no retry inside 5 s"
    );
    assert_eq!(lib.status()["songs"]["failed"], true);
    refused_thrice(&mut lib, &ctx, &Route::Songs);
    assert!(lib.denied);
    assert!(
        lib.tick(Instant::now() + RETRY * 2, true, || {}).is_empty(),
        "a 403 is not retried"
    );
    lib.tick(Instant::now(), false, || {});
    assert!(!lib.denied, "signing out clears it");
}

#[test]
fn opening_another_playlist_starts_its_tracks_afresh() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(
        &ctx,
        &mut lib,
        &Route::Playlist("p.A".into()),
        600.0,
        vec![],
    );
    answer(
        &mut lib,
        &ctx,
        now,
        page(
            0..100,
            Some("/v1/me/library/playlists/p.A/tracks?offset=100"),
        ),
    );
    frame(
        &ctx,
        &mut lib,
        &Route::Playlist("p.B".into()),
        600.0,
        vec![],
    );
    assert_eq!(lib.list(List::Tracks).items.len(), 0);
    let sent = lib.tick(now, true, || {});
    assert_eq!(
        sent.iter().map(|j| &j.call).collect::<Vec<_>>(),
        [&Call::PlaylistTracks("p.B".into())]
    );
}

#[test]
fn opening_a_library_album_reads_its_tracks_from_the_library() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Album("l.A".into()), 600.0, vec![]);
    let sent = lib.tick(now, true, || {});
    assert_eq!(
        sent.iter().map(|j| &j.call).collect::<Vec<_>>(),
        [&Call::AlbumTracks("l.A".into())]
    );
}

fn click(
    ctx: &egui::Context,
    lib: &mut Library,
    route: &Route,
    label: &str,
) -> (Vec<Action>, Vec<Nav>) {
    ctx.enable_accesskit();
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        show(ui, lib, (route, &mut None), (&mut vec![], &mut vec![]))
    });
    out.textures_delta.clear();
    let nodes = out
        .platform_output
        .accesskit_update
        .expect("an accesskit tree")
        .nodes;
    let row = nodes.iter().find(|(_, n)| n.label() == Some(label));
    let at = row
        .and_then(|(_, n)| n.bounds())
        .expect("the row on screen");
    let pos = Pos2::new((at.x0 + at.x1) as f32 / 2.0, (at.y0 + at.y1) as f32 / 2.0);
    let button = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        ctx,
        lib,
        route,
        600.0,
        vec![Event::PointerMoved(pos), button(true)],
    );
    frame(ctx, lib, route, 600.0, vec![button(false)])
}

#[test]
fn clicking_a_track_plays_the_playlist_from_that_track() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let route = Route::Playlist("p.LIST".into());
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    lib.lists[List::Tracks as usize].items = (0..10).map(item).collect();
    let (actions, _) = click(&ctx, &mut lib, &route, "Play Secret Song 3 · Secret Artist");
    let ids = (3..10).map(|i| format!("i.{i}")).collect();
    assert_eq!(actions, [Action::PlayIn(Queue::Songs(ids), 0)]);
}

fn named(ids: &[&str], next: Option<&str>) -> Reply {
    let items = ids.iter().enumerate().map(|(i, id)| Item {
        id: id.to_string(),
        ..item(i)
    });
    Reply::Page(Page {
        items: items.collect(),
        next: next.map(str::to_owned),
        total: Some(ids.len() as u32),
    })
}

#[test]
fn an_artist_reads_its_albums_then_each_albums_tracks_in_turn_and_starts_afresh_per_artist() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Artists, 600.0, vec![]);
    let next = "/v1/me/library/artists?offset=100";
    let first = answer(&mut lib, &ctx, now, page(0..100, Some(next)));
    assert_eq!(first, Call::Library(Shelf::Artists));
    frame(&ctx, &mut lib, &Route::Artists, 6000.0, vec![]);
    assert_eq!(
        answer(&mut lib, &ctx, now, page(100..137, None)),
        Call::Next(next.into())
    );
    assert_eq!(lib.status()["artists"]["loaded"], 137);
    let route = Route::Artist("i.136".into());
    let tracks = "/v1/me/library/albums/l.0/tracks?offset=100";
    let albums = "/v1/me/library/artists/i.136/albums?offset=1";
    let steps = [
        (
            Call::ArtistAlbums("i.136".into()),
            named(&["l.0"], Some(albums)),
        ),
        (Call::Next(albums.into()), named(&["l.1"], None)),
        (Call::AlbumTracks("l.0".into()), page(0..100, Some(tracks))),
        (Call::Next(tracks.into()), page(100..120, None)),
        (Call::AlbumTracks("l.1".into()), page(120..130, None)),
    ];
    for (call, reply) in steps {
        frame(&ctx, &mut lib, &route, 6000.0, vec![]);
        assert_eq!(answer(&mut lib, &ctx, now, reply), call);
    }
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    assert!(lib.tick(now, true, || {}).is_empty(), "every album is read");
    let status = lib.status();
    assert_eq!(status["artistSongs"]["loaded"], 130);
    assert_eq!(status["artistSongs"]["more"], false);
    frame(&ctx, &mut lib, &Route::Artist("r.B".into()), 600.0, vec![]);
    assert_eq!(lib.list(List::ArtistSongs).items.len(), 0);
    let sent = lib.tick(now, true, || {});
    assert_eq!(
        sent.iter().map(|j| &j.call).collect::<Vec<_>>(),
        [
            &Call::ArtistAlbums("r.B".into()),
            &Call::LibraryArtist("r.B".into())
        ],
        "a deep link fetches the artist for its heading"
    );
    let name = sent.into_iter().nth(1).unwrap();
    (name.done)(Ok(named(&["r.B"], None)));
    lib.receive(&ctx);
    let shown = texts(&ctx, &mut lib, &Route::Artist("r.B".into()));
    assert!(shown.iter().any(|t| t == "Secret Song 0"), "{shown:?}");
}

#[test]
fn artists_are_built_only_where_visible_as_names_without_art() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = (0..250).map(|i| art_item(i, "http://x")).collect();
    frame(&ctx, &mut lib, &Route::Artists, 600.0, vec![]);
    let (built, _, _) = lib.built;
    assert!(
        built > 0 && built <= 600 / ROW_PT as usize + 2,
        "{built} rows built"
    );
    assert_eq!(
        lib.art.drawn + lib.art.shown,
        0,
        "an artist row asks for art"
    );
}

#[test]
fn an_artist_opens_its_view_and_its_songs_and_albums_play_in_context() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = (0..5).map(item).collect();
    let (_, navs) = click(&ctx, &mut lib, &Route::Artists, "Open artist Secret Song 2");
    assert_eq!(navs, [Nav::Go(Route::Artist("i.2".into()))]);
    let route = Route::Artist("i.2".into());
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    lib.lists[List::ArtistAlbums as usize].items = (7..9).map(item).collect();
    lib.lists[List::ArtistSongs as usize].items = (0..10).map(item).collect();
    let (album, _) = click(&ctx, &mut lib, &route, "Play album Secret Song 8");
    assert_eq!(album, [Action::PlayIn(Queue::Album("i.8".into()), 0)]);
    let (song, _) = click(&ctx, &mut lib, &route, "Play Secret Song 4 · Secret Artist");
    assert_eq!(song, [Action::PlayIn(Queue::Songs(ids(4..10)), 0)]);
    lib.started(1);
    frame(
        &ctx,
        &mut lib,
        &Route::Artist("r.OTHER".into()),
        600.0,
        vec![],
    );
    lib.lists[List::ArtistSongs as usize].items = (0..20).map(item).collect();
    assert_eq!(lib.top_up(&window(0, 6, 4..10), LIVE, Instant::now()), None);
    assert!(
        lib.follow.is_none(),
        "another artist's songs never top up the queue"
    );
}

#[test]
fn signing_out_clears_the_artists_and_a_403_on_an_artist_asks_for_sign_in() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let route = Route::Artist("i.0".into());
    lib.lists[List::Artists as usize].items = (0..5).map(item).collect();
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    answer(&mut lib, &ctx, now, named(&["l.0"], None));
    answer(&mut lib, &ctx, now, page(0..10, None));
    let status = lib.status().to_string();
    assert!(
        !status.contains("Secret") && !status.contains("l.0"),
        "{status}"
    );
    assert!(lib.tick(now, false, || {}).is_empty());
    for list in [List::Artists, List::ArtistAlbums, List::ArtistSongs] {
        assert_eq!(lib.list(list).items.len(), 0, "{list:?} survived sign-out");
    }
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    refused_thrice(&mut lib, &ctx, &route);
    assert!(lib.denied && lib.tick(now + RETRY, true, || {}).is_empty());
}

#[test]
fn a_library_403_that_clears_on_retry_is_not_a_sign_out() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    for n in 1..=2 {
        let job = lib.tick(Instant::now() + RETRY * n, true, || {}).remove(0);
        (job.done)(Err(Error::SignInNeeded));
        lib.receive(&ctx);
        frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
        assert!(!lib.denied, "refusal {n}");
    }
    answer(
        &mut lib,
        &ctx,
        Instant::now() + RETRY * 3,
        page(0..10, None),
    );
    assert!(!lib.denied && lib.list(List::Songs).items.len() == 10);
}

fn texts(ctx: &egui::Context, lib: &mut Library, route: &Route) -> Vec<String> {
    ctx.enable_accesskit();
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        show(ui, lib, (route, &mut None), (&mut vec![], &mut vec![]))
    });
    out.textures_delta.clear();
    let nodes = out.platform_output.accesskit_update.expect("a tree").nodes;
    let text = |n: &egui::accesskit::Node| n.label().or(n.value()).map(str::to_owned);
    nodes.iter().filter_map(|(_, n)| text(n)).collect()
}

fn fail(lib: &mut Library, ctx: &egui::Context, now: Instant, why: Error) -> Vec<Call> {
    let jobs = lib.tick(now, true, || {});
    let calls = jobs.iter().map(|j| j.call.clone()).collect();
    for job in jobs {
        (job.done)(Err(why.clone()));
    }
    lib.receive(ctx);
    calls
}

#[test]
fn a_failing_albums_call_is_not_retried_once_the_artist_view_is_left() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = (0..5).map(item).collect();
    frame(&ctx, &mut lib, &Route::Artist("i.0".into()), 600.0, vec![]);
    let mut sent = Vec::new();
    for window in 0..30 {
        let at = Instant::now() + (RETRY + Duration::from_millis(5)) * window;
        sent.extend(fail(&mut lib, &ctx, at, Error::Http(500, "x".into())));
    }
    assert_eq!(sent, [Call::ArtistAlbums("i.0".into())]);
}

#[test]
fn an_unreadable_album_is_skipped_and_counted_while_a_network_error_is_retried() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let route = Route::Artist("i.0".into());
    lib.lists[List::Artists as usize].items = (0..5).map(item).collect();
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    answer(&mut lib, &ctx, now, named(&["l.0", "l.1", "l.2"], None));
    let step = |lib: &mut Library, at: Instant, why: Error| {
        frame(&ctx, lib, &route, 600.0, vec![]);
        fail(lib, &ctx, at, why)
    };
    let gone = Error::Http(404, "Resource Not Found".into());
    assert_eq!(step(&mut lib, now, gone), [Call::AlbumTracks("l.0".into())]);
    let down = Error::Network("down".into());
    assert_eq!(
        step(&mut lib, now, down.clone()),
        [Call::AlbumTracks("l.1".into())]
    );
    assert_eq!(step(&mut lib, Instant::now(), down), []);
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    let retried = answer(&mut lib, &ctx, Instant::now() + RETRY, page(0..3, None));
    assert_eq!(retried, Call::AlbumTracks("l.1".into()));
    let decode = Error::Decode("bad".into());
    assert_eq!(
        step(&mut lib, now, decode),
        [Call::AlbumTracks("l.2".into())]
    );
    let songs = &lib.status()["artistSongs"];
    assert_eq!(
        (songs["loaded"].as_u64(), songs["skipped"].as_u64()),
        (Some(3), Some(2))
    );
    assert_eq!(songs["more"], false);
    assert!(
        texts(&ctx, &mut lib, &route)
            .iter()
            .any(|t| t == "2 albums couldn't be read.")
    );
}

#[test]
fn a_rate_limit_waits_its_retry_after_but_never_less_than_the_retry() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    for (seconds, wait) in [(60, Duration::from_secs(60)), (1, RETRY)] {
        let now = Instant::now();
        assert_eq!(
            fail(&mut lib, &ctx, now, Error::RateLimited(seconds)).len(),
            1
        );
        lib.lists[List::Songs as usize].wanted = true;
        assert!(
            lib.tick(now + wait - Duration::from_millis(50), true, || {})
                .is_empty()
        );
        assert_eq!(
            lib.tick(now + wait + Duration::from_millis(50), true, || {})
                .len(),
            1
        );
        (
            lib.lists[List::Songs as usize].busy_until,
            lib.lists[List::Songs as usize].wanted,
        ) = (None, true);
    }
}

#[test]
fn rows_built_counts_both_artist_views_in_the_frame() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = (0..5).map(item).collect();
    let route = Route::Artist("i.0".into());
    frame(&ctx, &mut lib, &route, 1000.0, vec![]);
    lib.lists[List::ArtistAlbums as usize].items = (100..110).map(item).collect();
    lib.lists[List::ArtistSongs as usize].items = (0..60).map(item).collect();
    frame(&ctx, &mut lib, &route, 1000.0, vec![]);
    assert!(lib.art.shown > 10, "both views drew");
    assert_eq!((lib.built.0, lib.built.1), (lib.art.shown, lib.art.shown));
}

#[test]
fn songs_queue_from_the_click_onwards_and_albums_play_whole() {
    let items: Vec<_> = (0..250).map(item).collect();
    let Some(Action::PlayIn(Queue::Songs(ids), 0)) = super::play(List::Songs, &items, 120) else {
        panic!("a songs queue starting at its first entry");
    };
    assert_eq!((ids.len(), ids[0].as_str()), (super::QUEUE_AHEAD, "i.120"));
    let Some(Action::PlayIn(Queue::Songs(last), 0)) = super::play(List::Songs, &items, 249) else {
        panic!("a songs queue for the last row");
    };
    assert_eq!(last, ["i.249"]);
    assert_eq!(
        super::play(List::Albums, &items, 4),
        Some(Action::PlayIn(Queue::Album("i.4".into()), 0))
    );
    assert_eq!(super::play(List::Playlists, &items, 4), None);
}

#[test]
fn signed_out_sends_nothing_clears_the_library_and_drops_late_replies() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    assert!(lib.tick(now, false, || {}).is_empty());
    let late = lib.tick(now, true, || {}).remove(0);
    assert!(lib.tick(now, false, || {}).is_empty());
    (late.done)(Ok(page(0..100, None)));
    lib.receive(&ctx);
    assert_eq!(
        lib.list(List::Songs).items.len(),
        0,
        "a reply from before sign-out is dropped"
    );
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    answer(&mut lib, &ctx, now, page(0..5, None));
    assert!(lib.tick(now, false, || {}).is_empty());
    assert_eq!(
        lib.list(List::Songs).items.len(),
        0,
        "signing out clears what was shown"
    );
}

#[test]
fn library_status_and_source_carry_no_names_or_log_calls() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    answer(&mut lib, &ctx, now, page(0..100, None));
    let status = lib.status().to_string();
    assert!(
        !status.contains("Secret") && !status.contains("i.1"),
        "{status}"
    );
    let source = include_str!("../library.rs");
    for logger in ["println!", "eprintln!", "log::", "dbg!"] {
        assert!(!source.contains(logger), "library.rs uses {logger}");
    }
}

fn window(at: usize, length: usize, ids: std::ops::Range<usize>) -> Snapshot {
    let mut snap = Snapshot::default();
    let item = |i| QueueItem {
        id: format!("i.{i}"),
        title: None,
        duration: None,
    };
    (snap.queue_position, snap.queue_length) = (Some(at), length);
    snap.queue = ids.map(item).collect();
    snap
}

const LIVE: (u64, bool) = (1, true);

fn songs(range: std::ops::Range<usize>) -> (egui::Context, Library) {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let next = (range.end < 300).then_some("/v1/me/library/songs?offset=next");
    answer(&mut lib, &ctx, now, page(range, next));
    (ctx, lib)
}

fn ids(r: std::ops::Range<usize>) -> Vec<String> {
    r.map(|i| format!("i.{i}")).collect()
}

#[test]
fn a_songs_window_tops_up_16_at_a_time_near_its_end_up_to_a_hard_cap() {
    let ((ctx, mut lib), now) = (songs(0..120), Instant::now());
    let clicked = lib.play_from(List::Songs, 0);
    assert_eq!(clicked, Some(Action::PlayIn(Queue::Songs(ids(0..100)), 0)));
    lib.started(1);
    assert_eq!(
        lib.top_up(&window(88, 100, 88..100), LIVE, now),
        None,
        "11 ahead"
    );
    let near = window(89, 100, 89..100);
    assert_eq!(lib.top_up(&near, (1, false), now), None, "one in flight");
    assert_eq!(
        lib.top_up(&near, LIVE, now),
        Some(Action::TopUp(ids(100..116)))
    );
    assert_eq!(
        lib.top_up(&near, LIVE, now),
        None,
        "no second batch before the first lands"
    );
    let tail = lib.top_up(&window(110, 116, 110..116), LIVE, now);
    assert_eq!(tail, Some(Action::TopUp(ids(116..120))));
    assert_eq!(lib.top_up(&window(110, 120, 110..120), LIVE, now), None);
    let more = answer(&mut lib, &ctx, now, page(120..300, None));
    assert_eq!(more, Call::Next("/v1/me/library/songs?offset=next".into()));
    let paged = lib.top_up(&window(110, 120, 110..120), LIVE, now);
    assert_eq!(paged, Some(Action::TopUp(ids(120..136))));
    let short = window(QUEUE_CAP - 6, QUEUE_CAP - 3, 133..136);
    assert_eq!(
        lib.top_up(&short, LIVE, now),
        Some(Action::TopUp(ids(136..139)))
    );
    let full = window(QUEUE_CAP - 3, QUEUE_CAP, 136..139);
    assert_eq!(lib.top_up(&full, LIVE, now), None, "the cap is hard");
}

#[test]
fn a_top_up_that_never_lands_is_resent_with_backoff_then_given_up_with_a_notice() {
    let ((_ctx, mut lib), t0) = (songs(0..150), Instant::now());
    lib.play_from(List::Songs, 0);
    lib.started(1);
    let (near, batch) = (window(89, 100, 89..100), Some(Action::TopUp(ids(100..116))));
    assert_eq!(lib.top_up(&near, LIVE, t0), batch);
    let at = |s| t0 + Duration::from_secs(s);
    assert_eq!(lib.top_up(&near, LIVE, at(4)), None);
    assert_eq!(lib.top_up(&near, LIVE, at(5)), batch);
    assert_eq!(lib.top_up(&near, LIVE, at(34)), None);
    assert_eq!(lib.top_up(&near, LIVE, at(35)), batch);
    assert_eq!(lib.top_up(&near, LIVE, at(95)), batch);
    assert!(!lib.top_up_failed);
    assert_eq!(lib.top_up(&near, LIVE, at(500)), None);
    assert!(lib.top_up_failed, "the user is told");
}

#[test]
fn only_the_queue_the_user_started_from_the_list_is_topped_up() {
    let ((ctx, mut lib), now) = (songs(0..150), Instant::now());
    let near = window(89, 100, 89..100);
    lib.play_from(List::Songs, 0);
    assert_eq!(lib.top_up(&near, LIVE, now), None, "not yet applied");
    lib.started(1);
    let cli_play = window(0, 1, 42..43);
    assert_eq!(
        lib.top_up(&cli_play, (2, true), now),
        None,
        "`bramley play i.42`"
    );
    assert_eq!(lib.top_up(&near, LIVE, now), None, "and it stays dropped");
    lib.play_from(List::Songs, 0);
    lib.started(3);
    frame(&ctx, &mut lib, &Route::Albums, 600.0, vec![]);
    answer(&mut lib, &ctx, now, page(500..510, None));
    lib.play_from(List::Albums, 0);
    assert_eq!(lib.top_up(&near, (3, true), now), None, "an album click");
    lib.play_from(List::Songs, 0);
    lib.started(4);
    lib.reset();
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    answer(&mut lib, &ctx, now, page(0..150, None));
    assert_eq!(
        lib.top_up(&near, (4, true), now),
        None,
        "signed out and back in"
    );
}

#[test]
fn a_window_stops_following_a_playlist_that_was_replaced() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    for playlist in ["p.1", "p.2"] {
        frame(
            &ctx,
            &mut lib,
            &Route::Playlist(playlist.into()),
            600.0,
            vec![],
        );
        answer(&mut lib, &ctx, now, page(0..150, None));
        if playlist == "p.1" {
            lib.play_from(List::Tracks, 0);
            lib.started(1);
        }
    }
    assert_eq!(lib.top_up(&window(95, 100, 95..100), LIVE, now), None);
}

#[test]
fn scroll_end_pages_in_the_whole_list_then_lets_go_and_top_and_a_wheel_end_it() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let (route, mut scroll) = (Route::Playlist("p.1".into()), Some(Edge::End));
    let shown = |lib: &mut Library, scroll: &mut Option<Edge>, events| {
        scrolled(&ctx, lib, (&route, scroll), 600.0, events);
        lib.built.2
    };
    shown(&mut lib, &mut scroll, vec![]);
    for (range, next) in [
        (0..100, Some("n.1")),
        (100..200, Some("n.2")),
        (200..250, None),
    ] {
        answer(&mut lib, &ctx, now, page(range, next));
        shown(&mut lib, &mut scroll, vec![]);
    }
    assert_eq!(scroll, Some(Edge::End), "let go before the end was shown");
    assert_eq!((shown(&mut lib, &mut scroll, vec![]), scroll), (250, None));
    scroll = Some(Edge::Top);
    shown(&mut lib, &mut scroll, vec![]);
    assert_eq!(
        (shown(&mut lib, &mut scroll, vec![]) < 20, scroll),
        (true, None)
    );
    scroll = Some(Edge::End);
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    };
    shown(&mut lib, &mut scroll, vec![wheel]);
    assert_eq!(
        scroll, None,
        "a wheel turn takes scrolling back from the pin"
    );
}

#[test]
fn a_pinned_scroll_end_repaints_only_while_a_page_can_be_sent() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let mut scroll = Some(Edge::End);
    let mut delay = |lib: &mut Library| {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            show(
                ui,
                lib,
                (&Route::Songs, &mut scroll),
                (&mut vec![], &mut vec![]),
            )
        });
        out.textures_delta.clear();
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
    };
    assert_eq!(delay(&mut lib), Duration::ZERO, "the first page is due");
    let job = lib.tick(now, true, || {}).remove(0);
    (job.done)(Err(Error::Network("down".into())));
    lib.receive(&egui::Context::default());
    let seen: Vec<_> = (0..4).map(|_| delay(&mut lib)).collect();
    assert!(
        seen[3] > Duration::from_secs(1),
        "a failed page waits its backoff: {seen:?}"
    );
}

type Nodes = Vec<(String, egui::accesskit::Node)>;

fn sidebar(
    ctx: &egui::Context,
    lib: &mut Library,
    route: &Route,
    height: f32,
    events: Vec<Event>,
) -> (Nodes, Vec<Nav>) {
    ctx.enable_accesskit();
    let mut navs = Vec::new();
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(220.0, height))),
        events,
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        crate::ui::sidebar::show(ui, route, lib, &mut navs, true);
    });
    out.textures_delta.clear();
    let tree = out.platform_output.accesskit_update.expect("a tree");
    let nodes = tree.nodes.into_iter().map(|(_, n)| n);
    let named = nodes.filter_map(|n| Some((n.label().or(n.value())?.to_owned(), n)));
    (named.collect(), navs)
}

fn bounds(nodes: &Nodes, label: &str) -> egui::accesskit::Rect {
    let node = nodes.iter().find(|(l, _)| l == label);
    node.and_then(|(_, n)| n.bounds())
        .unwrap_or_else(|| panic!("no {label} in the sidebar"))
}

fn press(ctx: &egui::Context, lib: &mut Library, route: &Route, label: &str) -> (Nodes, Vec<Nav>) {
    let at = bounds(&sidebar(ctx, lib, route, 600.0, vec![]).0, label);
    let pos = Pos2::new((at.x0 + at.x1) as f32 / 2.0, (at.y0 + at.y1) as f32 / 2.0);
    let button = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let (_, mut navs) = sidebar(
        ctx,
        lib,
        route,
        600.0,
        vec![Event::PointerMoved(pos), button(true)],
    );
    let (_, released) = sidebar(ctx, lib, route, 600.0, vec![button(false)]);
    navs.extend(released);
    (sidebar(ctx, lib, route, 600.0, vec![]).0, navs)
}

fn playlists(n: usize) -> Library {
    let mut lib = Library::default();
    lib.lists[List::Playlists as usize].items = (0..n).map(item).collect();
    lib
}

#[test]
fn the_sidebar_nests_playlists_under_the_playlists_link_after_albums_and_songs() {
    let (ctx, mut lib) = (egui::Context::default(), playlists(3));
    let route = Route::Playlist("i.1".into());
    let (nodes, _) = sidebar(&ctx, &mut lib, &route, 600.0, vec![]);
    let order = [
        "Artists",
        "Albums",
        "Songs",
        "Playlists",
        "Secret Song 0",
        "Secret Song 1",
        "Secret Song 2",
        "Sign out",
    ];
    let tops: Vec<f64> = order.iter().map(|l| bounds(&nodes, l).y0).collect();
    assert!(tops.is_sorted_by(|a, b| a < b), "{order:?} at {tops:?}");
    let link = bounds(&nodes, "Playlists");
    assert!(
        bounds(&nodes, "Secret Song 0").x0 > link.x0,
        "rows are indented"
    );
    let selected: Vec<_> = nodes
        .iter()
        .filter(|(l, n)| {
            l.starts_with("Secret") && n.toggled() == Some(egui::accesskit::Toggled::True)
        })
        .map(|(l, _)| l.as_str())
        .collect();
    assert_eq!(selected, ["Secret Song 1"]);
}

#[test]
fn the_chevron_folds_and_unfolds_the_playlists_and_the_link_still_opens_the_page() {
    let (ctx, mut lib) = (egui::Context::default(), playlists(3));
    let rows = |nodes: &Nodes| {
        nodes
            .iter()
            .filter(|(l, _)| l.starts_with("Secret"))
            .count()
    };
    let (nodes, navs) = press(&ctx, &mut lib, &Route::Home, "Collapse playlists");
    assert_eq!((rows(&nodes), navs), (0, vec![]));
    assert!(bounds(&nodes, "Playlists").y0 > bounds(&nodes, "Songs").y1);
    let (nodes, navs) = press(&ctx, &mut lib, &Route::Home, "Expand playlists");
    assert_eq!((rows(&nodes), navs), (3, vec![]));
    let (nodes, navs) = press(&ctx, &mut lib, &Route::Home, "Playlists");
    assert_eq!((rows(&nodes), navs), (3, vec![Nav::Go(Route::Playlists)]));
}

#[test]
fn hundreds_of_playlists_still_leave_albums_songs_and_sign_out_on_screen() {
    let (ctx, mut lib) = (egui::Context::default(), playlists(300));
    let settle = |_| sidebar(&ctx, &mut lib, &Route::Home, 420.0, vec![]);
    let (nodes, _) = (0..3).map(settle).last().unwrap();
    let (songs, out) = (bounds(&nodes, "Songs"), bounds(&nodes, "Sign out"));
    let footer = nodes.iter().find(|(l, _)| l == "Signed in to Apple Music");
    let footer = footer
        .and_then(|(_, n)| n.bounds())
        .expect("the signed-in line");
    assert!(bounds(&nodes, "Albums").y1 <= songs.y0);
    let rows = nodes.iter().filter(|(l, _)| l.starts_with("Secret"));
    let tops: Vec<f64> = rows.filter_map(|(_, n)| n.bounds()).map(|r| r.y0).collect();
    let clipped = tops.iter().filter(|top| **top >= footer.y0).count();
    assert!(songs.y1 <= footer.y0, "{songs:?} {footer:?}");
    assert!(tops.iter().any(|top| *top > songs.y1 && *top < footer.y0));
    assert!(
        clipped <= 1,
        "{clipped} rows begin below the footer {footer:?}"
    );
    assert!(footer.y1 <= out.y0 && out.y1 <= 420.0, "{footer:?} {out:?}");
}

#[test]
fn the_footer_stays_below_the_playlist_rows_and_a_sliver_of_list_is_never_drawn_over_it() {
    for height in [300.0, 320.0, 340.0, 380.0, 420.0, 720.0] {
        let (ctx, mut lib) = (egui::Context::default(), playlists(300));
        let settle = |_| sidebar(&ctx, &mut lib, &Route::Home, height, vec![]);
        let (nodes, _) = (0..3).map(settle).last().unwrap();
        let footer = bounds(&nodes, "Signed in to Apple Music");
        let tops = nodes.iter().filter(|(l, _)| l.starts_with("Secret"));
        let tops: Vec<f64> = tops.filter_map(|(_, n)| n.bounds()).map(|r| r.y0).collect();
        let clipped = tops.iter().filter(|top| **top >= footer.y0).count();
        assert!(
            clipped <= 2,
            "{height}: {clipped} rows begin below the footer {footer:?}"
        );
        let room = footer.y0 - bounds(&nodes, "Playlists").y1;
        assert_eq!(
            tops.is_empty(),
            room < 25.0,
            "{height}: {room} left for {} rows",
            tops.len()
        );
        assert!(
            bounds(&nodes, "Sign out").y1 <= f64::from(height),
            "{height}: sign out is cut off"
        );
    }
}

fn lettered(letters: std::ops::Range<u8>) -> Vec<Item> {
    let named = |letter: u8, n: u8| Item {
        id: format!("i.{letter}{n}"),
        name: Some(format!("{}x{n}", letter as char)),
        ..item(0)
    };
    letters
        .flat_map(|l| (0..10).map(move |n| named(l, n)))
        .collect()
}

fn text(t: &str) -> Event {
    Event::Text(t.into())
}

fn on(
    (ctx, lib): (&egui::Context, &mut Library),
    (route, verb): (Route, &str),
    time: f64,
    events: Vec<Event>,
) -> Vec<String> {
    ctx.enable_accesskit();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let mut input = RawInput {
        screen_rect: Some(screen),
        time: Some(time),
        events,
        ..RawInput::default()
    };
    crate::ui::keys::take(ctx, &mut input);
    let mut out = ctx.run_ui(input, |ui| {
        show(ui, lib, (&route, &mut None), (&mut vec![], &mut vec![]))
    });
    out.textures_delta.clear();
    crate::ui::cursor::settle(ctx);
    let nodes = out.platform_output.accesskit_update.expect("a tree").nodes;
    let labels = nodes.iter().filter_map(|(_, n)| n.label());
    let verb = format!("{verb} ");
    labels
        .filter_map(|l| l.strip_prefix(&verb))
        .map(str::to_owned)
        .collect()
}

fn at(ctx: &egui::Context, lib: &mut Library, time: f64, events: Vec<Event>) -> Vec<String> {
    on((ctx, lib), (Route::Artists, "Open artist"), time, events)
}

fn names(shown: &[String]) -> Vec<&str> {
    shown.iter().map(String::as_str).collect()
}

#[test]
fn typing_a_letter_or_a_prefix_jumps_to_the_first_matching_artist() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let mut items = lettered(b'A'..b'Z' + 1);
    items.insert(
        20,
        Item {
            id: "i.mid".into(),
            name: Some("Cmx".into()),
            ..item(0)
        },
    );
    lib.lists[List::Artists as usize].items = items;
    let shown = at(&ctx, &mut lib, 0.0, vec![]);
    assert!(names(&shown).contains(&"Ax0") && !names(&shown).contains(&"Mx0"));
    at(&ctx, &mut lib, 1.0, vec![text("m")]);
    let shown = at(&ctx, &mut lib, 1.1, vec![]);
    assert!(names(&shown).contains(&"Mx0") && !names(&shown).contains(&"Ax0"));
    at(&ctx, &mut lib, 1.4, vec![text("x7")]);
    assert!(names(&at(&ctx, &mut lib, 1.5, vec![])).contains(&"Mx7"));
    at(&ctx, &mut lib, 5.0, vec![text("c")]);
    let shown = at(&ctx, &mut lib, 5.1, vec![]);
    assert!(
        names(&shown).contains(&"Cx0"),
        "a pause starts a new prefix"
    );
}

#[test]
fn typing_ignores_punctuation_and_spaces_in_the_names_it_matches() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let mut items = lettered(b'A'..b'Z' + 1);
    for (at, name) in [(100, "Mled Zeppelin"), (200, "AC/DC")] {
        let named = Item {
            id: format!("i.{at}"),
            name: Some(name.into()),
            ..item(0)
        };
        items.insert(at, named);
    }
    lib.lists[List::Artists as usize].items = items;
    let first = at(&ctx, &mut lib, 0.0, vec![]);
    assert!(!names(&first).contains(&"Mled Zeppelin"), "{first:?}");
    for (time, letter) in [(1.0, "m"), (1.1, "l"), (1.2, "e"), (1.3, "d")] {
        at(&ctx, &mut lib, time, vec![text(letter)]);
    }
    let shown = at(&ctx, &mut lib, 1.4, vec![]);
    assert!(names(&shown).contains(&"Mled Zeppelin"), "{shown:?}");
    at(&ctx, &mut lib, 5.0, vec![text("acdc")]);
    let shown = at(&ctx, &mut lib, 5.1, vec![]);
    assert!(names(&shown).contains(&"AC/DC"), "{shown:?}");
}

#[test]
fn j_moves_and_a_started_prefix_still_jumps() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let mut items = lettered(b'A'..b'Z' + 1);
    items.insert(
        120,
        Item {
            name: Some("Mjolnir".into()),
            ..item(0)
        },
    );
    lib.lists[List::Artists as usize].items = items;
    at(&ctx, &mut lib, 0.5, vec![text("j")]);
    assert!(lib.typed.0.is_empty(), "a bare j moves, it does not type");
    at(&ctx, &mut lib, 1.0, vec![text("m")]);
    at(&ctx, &mut lib, 1.1, vec![text("j")]);
    assert_eq!(lib.typed.0, "mj");
    assert!(names(&at(&ctx, &mut lib, 1.2, vec![])).contains(&"Mjolnir"));
}

#[test]
fn right_past_the_last_column_enters_the_strip_and_enter_jumps() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = lettered(b'A'..b'Z' + 1);
    at(&ctx, &mut lib, 0.0, vec![]);
    for (time, letter) in (1..)
        .map(|n| n as f64)
        .zip(["l", "j", "j", "j", "j", "j", "j", "j"])
    {
        at(&ctx, &mut lib, time, vec![text(letter)]);
    }
    let enter = crate::tests::pressed(&ctx, Some((egui::Key::Enter, false))).events;
    at(&ctx, &mut lib, 20.0, enter);
    let shown = at(&ctx, &mut lib, 21.0, vec![]);
    assert!(names(&shown).contains(&"Hx0") && !names(&shown).contains(&"Ax0"));
    assert!(lib.typed.0.is_empty());
}

#[test]
fn left_and_escape_leave_the_strip() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = lettered(b'A'..b'Z' + 1);
    at(&ctx, &mut lib, 0.0, vec![]);
    let id = egui::Id::new(("rows", List::Artists as usize));
    let strip = || crate::ui::cursor::spans(&ctx, id, 26);
    for (time, escape) in [(2.0, false), (4.0, true)] {
        at(&ctx, &mut lib, time - 1.0, vec![text("l")]);
        assert_eq!(strip(), Some(0));
        let esc = || crate::tests::pressed(&ctx, Some((egui::Key::Escape, false))).events;
        at(
            &ctx,
            &mut lib,
            time,
            if escape { esc() } else { vec![text("h")] },
        );
        assert_eq!(strip(), None);
    }
}

#[test]
fn up_down_j_k_walk_the_strip_clamped_enter_jumps_and_a_jump_ends_the_prefix() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = lettered(b'A'..b'Z' + 1);
    let id = egui::Id::new(("rows", List::Artists as usize));
    let key = |k| crate::tests::pressed(&ctx, Some((k, false))).events;
    let mut walk = |time, events| {
        at(&ctx, &mut lib, time, events);
        crate::ui::cursor::spans(&ctx, id, 26)
    };
    assert_eq!(walk(1.0, vec![text("l")]), Some(0));
    assert_eq!(walk(2.0, vec![text("k")]), Some(0));
    assert_eq!(walk(3.0, vec![text("j")]), Some(1));
    for time in 6..40 {
        walk(f64::from(time), vec![text("j")]);
    }
    assert_eq!(walk(40.0, key(egui::Key::ArrowDown)), Some(25));
    assert_eq!(walk(41.0, key(egui::Key::ArrowUp)), Some(24));
    walk(42.0, vec![]);
    walk(42.1, vec![text("m")]);
    walk(42.2, key(egui::Key::Enter));
    walk(42.3, vec![text("j")]);
    assert!(lib.typed.0.is_empty(), "a strip jump ends the prefix");
    assert!(names(&at(&ctx, &mut lib, 44.0, vec![])).contains(&"Yx0"));
}

#[test]
fn the_cursor_lands_on_the_true_last_row_once_a_pinned_scroll_end_has_paged_in() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    ctx.enable_accesskit();
    let (route, mut scroll) = (Route::Playlist("p.1".into()), Some(Edge::End));
    let mut shown = |lib: &mut Library| {
        let input = crate::tests::pressed(&ctx, None);
        let mut out = ctx.run_ui(input, |ui| {
            show(ui, lib, (&route, &mut scroll), (&mut vec![], &mut vec![]))
        });
        out.textures_delta.clear();
        crate::ui::cursor::settle(&ctx);
        crate::ui::tests::focus_label(&out.platform_output.accesskit_update.expect("a tree"))
    };
    shown(&mut lib);
    for (range, next) in [(0..100, Some("n.1")), (100..250, None)] {
        answer(&mut lib, &ctx, now, page(range, next));
        shown(&mut lib);
    }
    let landed = (0..3).map(|_| shown(&mut lib)).last().flatten();
    assert_eq!(landed, played(249));
}

#[test]
fn the_index_strip_jumps_to_a_letter() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = lettered(b'A'..b'Z' + 1);
    click(&ctx, &mut lib, &Route::Artists, "Jump to Q");
    let shown = at(&ctx, &mut lib, 1000.0, vec![]);
    assert!(names(&shown).contains(&"Qx0") && !names(&shown).contains(&"Ax0"));
}

#[test]
fn typing_is_left_alone_for_space_shortcuts_the_search_box_and_other_lists() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Artists as usize].items = lettered(b'A'..b'Z' + 1);
    let shortcut = egui::Modifiers::COMMAND;
    let held = RawInput {
        events: vec![Event::ModifiersChanged(shortcut), text("l")],
        ..RawInput::default()
    };
    let mut field = String::new();
    let id = crate::ui::content::search_id();
    ctx.memory_mut(|m| m.request_focus(id));
    let mut typing = |ctx: &egui::Context, lib: &mut Library, input: RawInput| {
        let mut out = ctx.run_ui(input, |ui| {
            ui.add(egui::TextEdit::singleline(&mut field).id(id));
            show(
                ui,
                lib,
                (&Route::Artists, &mut None),
                (&mut vec![], &mut vec![]),
            )
        });
        out.textures_delta.clear();
    };
    typing(&ctx, &mut lib, RawInput::default());
    typing(
        &ctx,
        &mut lib,
        RawInput {
            events: vec![text("m")],
            ..RawInput::default()
        },
    );
    assert!(lib.typed.0.is_empty(), "the search box owns its keys");
    ctx.memory_mut(|m| m.surrender_focus(id));
    at(&ctx, &mut lib, 1.0, vec![text(" ")]);
    assert!(lib.typed.0.is_empty(), "space is the play toggle");
    let mut out = ctx.run_ui(held, |ui| {
        show(
            ui,
            &mut lib,
            (&Route::Artists, &mut None),
            (&mut vec![], &mut vec![]),
        )
    });
    out.textures_delta.clear();
    assert!(lib.typed.0.is_empty(), "a command chord is not typing");
    lib.lists[List::Songs as usize].items = (0..5).map(item).collect();
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![text("m")]);
    assert!(lib.typed.0.is_empty(), "only Artists and Albums jump");
}

#[test]
fn a_letter_not_loaded_yet_pages_on_until_it_arrives_then_stops_when_the_list_ends() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let list = &mut lib.lists[List::Artists as usize];
    (list.items, list.pages) = (lettered(b'A'..b'D'), 1);
    list.next = Some("/v1/me/library/artists?offset=30".into());
    at(&ctx, &mut lib, 1.0, vec![text("z")]);
    at(&ctx, &mut lib, 1.1, vec![]);
    let waiting = Some((List::Artists, "z".to_owned()));
    assert_eq!(lib.jump, waiting, "the jump waits for the page");
    let albums = &mut lib.lists[List::Albums as usize];
    (albums.items, albums.pages) = (lettered(b'A'..b'D'), 1);
    let shown = on(
        (&ctx, &mut lib),
        (Route::Albums, "Play album"),
        1.15,
        vec![],
    );
    assert!(names(&shown).contains(&"Ax0"), "Albums stays at the top");
    assert_eq!(lib.jump, waiting, "another list leaves it pending");
    let sent = lib.tick(now, true, || {});
    assert_eq!(
        sent.iter().map(|j| &j.call).collect::<Vec<_>>(),
        [&Call::Next("/v1/me/library/artists?offset=30".into())]
    );
    for job in sent {
        let more = lettered(b'Y'..b'[');
        let reply = Page {
            items: more,
            next: None,
            total: None,
        };
        (job.done)(Ok(Reply::Page(reply)));
    }
    lib.receive(&ctx);
    at(&ctx, &mut lib, 1.2, vec![]);
    assert!(lib.jump.is_none());
    assert!(names(&at(&ctx, &mut lib, 1.3, vec![])).contains(&"Zx0"));
    at(&ctx, &mut lib, 9.0, vec![text("q")]);
    at(&ctx, &mut lib, 9.1, vec![]);
    assert!(
        lib.jump.is_none(),
        "a finished list has no more to wait for"
    );
}

#[test]
fn a_catalog_artist_lists_its_albums_from_the_catalog_and_an_album_opens_its_view() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    let route = Route::Artist("136975".into());
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    let sent = lib.tick(now, true, || {});
    let calls: Vec<_> = sent.iter().map(|j| &j.call).collect();
    assert_eq!(calls, [&Call::CatalogArtistAlbums("136975".into())]);
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    assert!(
        lib.tick(now, true, || {}).is_empty(),
        "no library artist or songs call"
    );
    for job in sent {
        (job.done)(Ok(named(&["1440", "1441"], None)));
    }
    lib.receive(&ctx);
    assert!(texts(&ctx, &mut lib, &route).contains(&"Secret Artist".to_owned()));
    let (actions, navs) = click(&ctx, &mut lib, &route, "Open album Secret Song 1");
    assert_eq!(
        (actions, navs),
        (vec![], vec![Nav::Go(Route::Album("1441".into()))])
    );
}

#[test]
fn typing_on_albums_jumps_the_grid_to_the_row_of_the_first_match() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    lib.lists[List::Albums as usize].items = lettered(b'A'..b'Z' + 1);
    let albums = || (Route::Albums, "Play album");
    let shown = on((&ctx, &mut lib), albums(), 0.0, vec![]);
    assert!(names(&shown).contains(&"Ax0") && !names(&shown).contains(&"Mx0"));
    on((&ctx, &mut lib), albums(), 1.0, vec![text("m")]);
    let shown = on((&ctx, &mut lib), albums(), 1.1, vec![]);
    assert!(names(&shown).contains(&"Mx0") && !names(&shown).contains(&"Ax0"));
}

#[test]
fn a_changed_first_page_replaces_the_stale_one_in_place_and_an_old_session_is_ignored() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let mut job = lib.tick(now, true, || {}).remove(0);
    let refresh = job.refresh.take().expect("a library page revalidates");
    (job.done)(Ok(page(0..100, Some("/v1/me/library/songs?offset=100"))));
    lib.receive(&ctx);
    lib.lists[List::Songs as usize].wanted = true;
    answer(&mut lib, &ctx, now + DEADLINE, page(100..150, None));
    assert_eq!(lib.list(List::Songs).items.len(), 150);
    refresh(Ok(page(500..600, Some("/v1/me/library/songs?offset=100"))));
    lib.receive(&ctx);
    let songs = lib.list(List::Songs);
    let ids: Vec<_> = songs.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!((ids.len(), ids[0], ids[99]), (100, "i.500", "i.599"));
    assert!(songs.next.is_some(), "the later pages load again on demand");

    let mut lib = Library::default();
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let late = lib.tick(now, true, || {}).remove(0).refresh.unwrap();
    lib.reset();
    late(Ok(page(0..100, None)));
    lib.receive(&ctx);
    assert!(
        lib.list(List::Songs).items.is_empty(),
        "a refresh from before sign-out landed"
    );
}

#[test]
fn a_403_on_revalidation_reloads_the_list_live_and_the_403_rules_end_in_sign_out() {
    let (ctx, mut lib, now) = (egui::Context::default(), Library::default(), Instant::now());
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
    let mut job = lib.tick(now, true, || {}).remove(0);
    let refresh = job.refresh.take().unwrap();
    (job.done)(Ok(page(0..100, None)));
    lib.receive(&ctx);
    assert_eq!(lib.list(List::Songs).items.len(), 100);
    refresh(Err(Error::SignInNeeded));
    lib.receive(&ctx);
    assert!(
        lib.list(List::Songs).items.is_empty(),
        "the revoked page was dropped"
    );
    refused_thrice(&mut lib, &ctx, &Route::Songs);
    assert!(lib.denied);
}

#[test]
fn warm_asks_for_the_urls_and_sizes_the_library_view_asks_for() {
    let template = "https://a1.mzstatic.com/us/r/{w}x{h}bb.jpg";
    let item = Item {
        artwork: Some(template.into()),
        ..item(0)
    };
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        ctx.run_ui(RawInput::default(), |_| {})
            .textures_delta
            .clear();
        assert_eq!(ctx.pixels_per_point(), scale);
        let mut lib = Library::default();
        lib.art.get(&ctx, &item, ROW_PT);
        lib.art.get(&ctx, &item, super::CELL_PT);
        let asked: Vec<(String, u32)> = lib
            .art
            .wanted
            .iter()
            .map(|(_, url, px)| (url.clone(), *px))
            .collect();
        let sizes = crate::warm::sizes(scale);
        let warm = |px| (bramley_api::art_url(template, px).unwrap(), px);
        assert_eq!(asked, [warm(sizes.row), warm(sizes.cell)], "{scale}");
    }
}

fn strip_at(
    ctx: &egui::Context,
    lib: &mut Library,
    height: f32,
    events: Vec<Event>,
) -> Vec<(String, Rect)> {
    ctx.enable_accesskit();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, height));
    let input = RawInput {
        screen_rect: Some(screen),
        events,
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let route = (&Route::Artists, &mut None);
        show(ui, lib, route, (&mut vec![], &mut vec![]))
    });
    out.textures_delta.clear();
    let nodes = out.platform_output.accesskit_update.expect("a tree").nodes;
    let named = nodes.iter().filter_map(|(_, n)| {
        let b = n.bounds()?;
        let (min, max) = (
            Pos2::new(b.x0 as f32, b.y0 as f32),
            Pos2::new(b.x1 as f32, b.y1 as f32),
        );
        Some((n.label()?.to_owned(), Rect::from_min_max(min, max)))
    });
    named.collect()
}

fn settled_strip(ctx: &egui::Context, lib: &mut Library, height: f32) -> Vec<(char, Rect)> {
    let nodes = (0..3)
        .map(|_| strip_at(ctx, lib, height, vec![]))
        .last()
        .unwrap();
    let letter = |(label, rect): &(String, Rect)| {
        Some((label.strip_prefix("Jump to ")?.chars().next()?, *rect))
    };
    let mut letters: Vec<_> = nodes.iter().filter_map(letter).collect();
    letters.sort_by_key(|(l, _)| *l);
    letters
}

#[test]
fn the_a_to_z_strip_fits_any_height_keeping_a_and_z_and_thinning_to_fewer_letters() {
    for height in [720.0, 480.0, 300.0, 200.0, 120.0] {
        let mut lib = Library::default();
        lib.lists[List::Artists as usize].items = lettered(b'A'..b'C');
        let letters = settled_strip(&egui::Context::default(), &mut lib, height);
        let shown: String = letters.iter().map(|(l, _)| *l).collect();
        assert!(
            shown.starts_with('A') && shown.ends_with('Z'),
            "{height}: {shown}"
        );
        assert_eq!(shown.len() == 26, height >= 300.0, "{height}: {shown}");
        for (letter, rect) in &letters {
            assert!(
                rect.bottom() <= height + 0.5,
                "{height}: {letter} is cut off at {}",
                rect.bottom()
            );
        }
        let stacked = letters
            .windows(2)
            .all(|w| w[0].1.bottom() <= w[1].1.top() + 0.5);
        assert!(stacked, "{height}: letters overlap in {shown}");
    }
}

#[test]
fn a_letter_the_thin_strip_hides_still_jumps_through_its_neighbour_and_by_typing() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    let mut names = lettered(b'A'..b'B');
    names.extend(lettered(b'D'..b'E'));
    lib.lists[List::Artists as usize].items = names;
    let letters = settled_strip(&ctx, &mut lib, 200.0);
    let shown: String = letters.iter().map(|(l, _)| *l).collect();
    assert!(shown.contains('C') && !shown.contains('D'), "{shown}");
    let top = |nodes: &[(String, Rect)], name: &str| {
        let row = nodes.iter().find(|(l, _)| l.ends_with(name));
        row.map_or(f32::MAX, |(_, r)| r.top())
    };
    let c = letters
        .iter()
        .find(|(l, _)| *l == 'C')
        .expect("the C button")
        .1;
    let at = Pos2::new(790.0, c.center().y);
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let before = strip_at(&ctx, &mut lib, 200.0, vec![]);
    assert!(top(&before, "Dx0") > 200.0, "D starts below the fold");
    strip_at(
        &ctx,
        &mut lib,
        200.0,
        vec![Event::PointerMoved(at), button(true)],
    );
    strip_at(&ctx, &mut lib, 200.0, vec![button(false)]);
    let after = strip_at(&ctx, &mut lib, 200.0, vec![]);
    assert!(
        top(&after, "Dx0") < 60.0,
        "C jumped to D: {}",
        top(&after, "Dx0")
    );
    strip_at(&ctx, &mut lib, 200.0, vec![text("a")]);
    assert_eq!(lib.typed.0, "a", "every letter still types");
}

fn cursor_key(ctx: &egui::Context, lib: &mut Library, (route, key): (&Route, egui::Key)) -> Landed {
    let mut landed = Landed::default();
    for pressed in [true, false] {
        let mut input = RawInput::default();
        if pressed {
            input = crate::ui::tests::hooked(ctx, key);
        }
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 300.0)));
        let (mut actions, mut navs) = (Vec::new(), Vec::new());
        let mut out = ctx.run_ui(input, |ui| {
            show(ui, lib, (route, &mut None), (&mut actions, &mut navs))
        });
        out.textures_delta.clear();
        lib.art.flush(ctx);
        crate::ui::cursor::settle(ctx);
        let tree = out.platform_output.accesskit_update.expect("a tree");
        let built = tree.nodes.iter().filter_map(|(_, n)| n.label());
        landed.built = built.filter(|l| l.contains("Secret Song")).count();
        landed.focus = crate::ui::tests::focus_label(&tree);
        landed.emitted |= !actions.is_empty() || !navs.is_empty();
    }
    landed
}

fn played(n: usize) -> Option<String> {
    Some(format!("Play Secret Song {n} · Secret Artist"))
}

#[derive(Default)]
struct Landed {
    focus: Option<String>,
    built: usize,
    emitted: bool,
}

#[test]
fn thirty_downs_build_focus_and_announce_row_thirty() {
    let (ctx, mut lib) = songs(0..100);
    ctx.enable_accesskit();
    let mut landed = Landed::default();
    for _ in 0..30 {
        landed = cursor_key(&ctx, &mut lib, (&Route::Songs, egui::Key::ArrowDown));
    }
    assert_eq!(landed.focus, played(29));
}

#[test]
fn home_end_and_page_keys_move_by_list_and_viewport() {
    let (ctx, mut lib) = songs(0..100);
    ctx.enable_accesskit();
    let mut at = |key| cursor_key(&ctx, &mut lib, (&Route::Songs, key));
    assert_eq!(at(egui::Key::End).focus, played(99));
    let top = at(egui::Key::Home);
    assert_eq!(top.focus, played(0));
    let page = top.built;
    assert!((5..40).contains(&page), "a viewport builds {page} rows");
    let down = at(egui::Key::PageDown).focus;
    assert_eq!(down, played(page));
    assert_eq!(at(egui::Key::PageUp).focus, played(0));
}

#[test]
fn moving_focus_never_emits_an_action() {
    let (ctx, mut lib) = songs(0..100);
    ctx.enable_accesskit();
    let keys = [
        egui::Key::ArrowDown,
        egui::Key::End,
        egui::Key::PageUp,
        egui::Key::Home,
    ];
    assert!(
        keys.iter()
            .all(|k| !cursor_key(&ctx, &mut lib, (&Route::Songs, *k)).emitted)
    );
}

#[test]
fn enter_on_albums_plays_the_focused_album_and_a_repeated_enter_plays_nothing() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    frame(&ctx, &mut lib, &Route::Albums, 600.0, vec![]);
    answer(&mut lib, &ctx, Instant::now(), page(0..30, None));
    let mut press = |key, repeat| {
        let events = crate::tests::pressed(&ctx, Some((key, repeat))).events;
        let route = (&Route::Albums, &mut None);
        let emitted = scrolled(&ctx, &mut lib, route, 600.0, events);
        crate::ui::cursor::settle(&ctx);
        emitted
    };
    assert_eq!(press(egui::Key::ArrowDown, false), (vec![], vec![]));
    assert_eq!(press(egui::Key::Enter, true), (vec![], vec![]));
    let played = Action::PlayIn(Queue::Album("i.0".into()), 0);
    assert_eq!(press(egui::Key::Enter, false), (vec![played], vec![]));
}

#[test]
fn tab_reaches_the_artist_songs_list_and_down_then_moves_it() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    ctx.enable_accesskit();
    let route = Route::Artist("i.0".into());
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    lib.lists[List::ArtistAlbums as usize].items = (100..110).map(item).collect();
    lib.lists[List::ArtistSongs as usize].items = (0..60).map(item).collect();
    let down = (&route, egui::Key::ArrowDown);
    let albums = cursor_key(&ctx, &mut lib, down).focus.unwrap();
    assert!(albums.starts_with("Play album Secret Song 100"), "{albums}");
    crate::ui::cursor::cycle(&ctx, false, (false, false));
    let songs = cursor_key(&ctx, &mut lib, down).focus;
    assert_eq!(songs, played(0));
}
