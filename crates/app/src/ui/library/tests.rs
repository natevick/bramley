use std::time::Instant;

use bramley_api::{Call, Item, Kind, Page, Reply, Shelf};
use bramley_engine::Queue;
use egui::{Event, PointerButton, Pos2, RawInput, Rect, vec2};

use super::{ART_CAP, DEADLINE, Library, List, show};
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
        lib.art.frame = i as u64;
        let texture = ctx.load_texture(
            format!("t{i}"),
            egui::ColorImage::filled([2, 2], egui::Color32::RED),
            Default::default(),
        );
        lib.art.insert(format!("i.{i}"), texture);
        if i == ART_CAP - 1 {
            lib.art.textures.get_mut("i.0").unwrap().1 = i as u64;
        }
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

#[test]
fn clicking_a_track_plays_its_playlist_from_that_track() {
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
    assert_eq!(
        actions,
        [Action::PlayIn(Queue::Playlist("p.LIST".into()), 3)]
    );
}

#[test]
fn songs_play_in_a_window_around_the_click_and_albums_play_whole() {
    let items: Vec<_> = (0..250).map(item).collect();
    let Some(Action::PlayIn(Queue::Songs(ids), start)) = super::play(List::Songs, &items, 120, "")
    else {
        panic!("a songs queue");
    };
    assert_eq!(
        (ids.len(), ids[start].as_str(), ids[0].as_str()),
        (super::SONGS_WINDOW, "i.120", "i.70")
    );
    assert_eq!(
        super::play(List::Albums, &items, 4, ""),
        Some(Action::PlayIn(Queue::Album("i.4".into()), 0))
    );
    assert_eq!(super::play(List::Playlists, &items, 4, ""), None);
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
fn library_names_never_reach_the_status_or_a_log_line() {
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
