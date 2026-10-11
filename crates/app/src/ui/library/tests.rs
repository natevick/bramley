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
use crate::ui::nav::{Nav, Route};

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
    let (mut actions, mut navs) = (Vec::new(), Vec::new());
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, height));
    let input = RawInput {
        screen_rect: Some(screen),
        events,
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| show(ui, lib, route, (&mut actions, &mut navs)));
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
    let job = lib.tick(Instant::now() + RETRY, true, || {}).remove(0);
    (job.done)(Err(Error::SignInNeeded));
    lib.receive(&ctx);
    frame(&ctx, &mut lib, &Route::Songs, 600.0, vec![]);
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
fn clicking_a_track_plays_the_playlist_from_that_track() {
    let (ctx, mut lib) = (egui::Context::default(), Library::default());
    ctx.enable_accesskit();
    let route = Route::Playlist("p.LIST".into());
    frame(&ctx, &mut lib, &route, 600.0, vec![]);
    lib.lists[List::Tracks as usize].items = (0..10).map(item).collect();
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        show(ui, &mut lib, &route, (&mut vec![], &mut vec![]))
    });
    out.textures_delta.clear();
    let nodes = out
        .platform_output
        .accesskit_update
        .expect("an accesskit tree")
        .nodes;
    let row = nodes
        .iter()
        .find(|(_, n)| n.label() == Some("Play Secret Song 3 · Secret Artist"));
    let at = row.and_then(|(_, n)| n.bounds()).expect("row 3 on screen");
    let pos = Pos2::new((at.x0 + at.x1) as f32 / 2.0, (at.y0 + at.y1) as f32 / 2.0);
    let button = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        &ctx,
        &mut lib,
        &route,
        600.0,
        vec![Event::PointerMoved(pos), button(true)],
    );
    let (actions, _) = frame(&ctx, &mut lib, &route, 600.0, vec![button(false)]);
    let ids = (3..10).map(|i| format!("i.{i}")).collect();
    assert_eq!(actions, [Action::PlayIn(Queue::Songs(ids), 0)]);
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
