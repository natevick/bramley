use std::time::{Duration, Instant};

use bramley_api::{Call, Detail as Found, Error, Item, Page, Reply};
use bramley_engine::Queue;
use serde_json::json;

use super::{Detail, Model, State, container, play_from};
use crate::playback::Action;
use crate::ui::nav::Route;

fn item(id: &str, kind: &str, ms: u64) -> Item {
    let mut attributes = json!({ "name": format!("{kind} {id}"), "durationInMillis": ms,
        "artwork": { "url": "http://127.0.0.1:9/a/{w}x{h}bb.jpg" } });
    if !id.starts_with('u') {
        attributes["playParams"] = json!({ "id": id, "kind": "song" });
    }
    serde_json::from_value(json!({ "id": id, "type": kind, "attributes": attributes })).unwrap()
}

fn page(items: &[(&str, &str)], next: Option<&str>) -> Page {
    Page {
        items: items
            .iter()
            .map(|(id, kind)| item(id, kind, 61_000))
            .collect(),
        next: next.map(str::to_owned),
        total: None,
    }
}

fn found(tracks: Page) -> Found {
    Found {
        item: item("1", "albums", 0),
        tracks,
    }
}

#[test]
fn queue_places_count_only_what_musickit_keeps_and_totals_wait_for_every_page() {
    let first = [
        ("a", "songs"),
        ("u", "songs"),
        ("v", "music-videos"),
        ("b", "songs"),
    ];
    let mut model = Model::new(found(page(&first, Some("/next"))));
    assert_eq!(model.at, [Some(0), None, Some(2)]);
    assert_eq!((model.summary(), model.first_page), ("3+ songs".into(), 3));
    model.add(page(&[("c", "songs")], None));
    assert_eq!(model.at, [Some(0), None, Some(2), Some(3)]);
    assert_eq!(model.summary(), "4 songs, 4 min");
    let ids: Vec<String> = (0..super::MAX_TRACKS + 1)
        .map(|n| format!("s{n}"))
        .collect();
    let many: Vec<_> = ids.iter().map(|id| (id.as_str(), "songs")).collect();
    let capped = Model::new(found(page(&many, None)));
    assert_eq!(capped.summary(), "1000+ songs");
}

#[test]
fn a_click_plays_its_queue_place_and_past_musickits_first_page_a_window_of_songs() {
    let first = [("v", "music-videos"), ("u", "songs"), ("a", "songs")];
    let mut model = Model::new(found(page(&first, Some("/next"))));
    model.add(page(
        &[("b", "songs"), ("u2", "songs"), ("c", "songs")],
        None,
    ));
    let album = Queue::Album("1".into());
    assert_eq!(
        play_from(&album, &model, 1),
        Some(Action::PlayIn(album.clone(), 1))
    );
    assert_eq!(play_from(&album, &model, 0), None);
    let window = Queue::Songs(vec!["b".into(), "c".into()]);
    assert_eq!(
        play_from(&album, &model, 2),
        Some(Action::PlayIn(window, 0))
    );
    assert_eq!(container(&Route::Album("1".into())), Some(album));
    assert_eq!(container(&Route::Album("l.mine".into())), None);
    let catalog = Route::Playlist("pl.x".into());
    assert_eq!(container(&catalog), Some(Queue::Playlist("pl.x".into())));
    assert_eq!(container(&Route::Playlist("p.mine".into())), None);
}

#[test]
fn clicking_a_row_play_or_shuffle_sends_that_action() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    ctx.enable_accesskit();
    tick(&mut d, t0, true, &Route::Album("1".into()));
    let songs = [
        ("u", "songs"),
        ("v", "music-videos"),
        ("a", "songs"),
        ("b", "songs"),
    ];
    answer(&mut d, &ctx, Ok(Reply::Detail(found(page(&songs, None)))));
    let album = Queue::Album("1".into());
    let wanted = [
        ("Play songs b", Some(Action::PlayIn(album.clone(), 2))),
        ("Play all", Some(Action::PlayIn(album.clone(), 1))),
        ("Shuffle", Some(Action::Shuffle(album))),
        ("Play songs u", None),
    ];
    for (label, want) in wanted {
        let mut actions = Vec::new();
        let mut frame = |events| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| super::show(ui, &mut d, &mut actions));
            out.textures_delta.clear();
            out.platform_output.accesskit_update
        };
        let nodes = frame(Vec::new()).expect("an accesskit tree").nodes;
        let node = nodes.iter().find(|(_, n)| n.label() == Some(label));
        let disabled = node.is_some_and(|(_, n)| n.is_disabled());
        assert_eq!(disabled, want.is_none(), "{label} greyed out");
        let at = node.and_then(|(_, n)| n.bounds()).expect(label);
        let at = egui::pos2(
            ((at.x0 + at.x1) / 2.0) as f32,
            ((at.y0 + at.y1) / 2.0) as f32,
        );
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(at), button(true)]);
        frame(vec![button(false)]);
        assert_eq!(actions, Vec::from_iter(want), "{label}");
    }
}

fn tick(d: &mut Detail, now: Instant, signed_in: bool, route: &Route) -> Option<Call> {
    d.tick(now, signed_in, route, || {}).map(|job| job.call)
}

fn answer(d: &mut Detail, ctx: &egui::Context, reply: bramley_api::Result<Reply>) {
    d.tx.send((d.seq, reply, false)).unwrap();
    d.receive(ctx);
}

#[test]
fn opening_fetches_once_pages_the_rest_and_drops_replies_for_another_album() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    let (album, other) = (Route::Album("1".into()), Route::Album("2".into()));
    let stale = d.tick(t0, true, &other, || {}).unwrap();
    assert_eq!(
        tick(&mut d, t0, true, &album),
        Some(Call::Album("1".into()))
    );
    (stale.done)(Ok(Reply::Detail(found(page(&[("z", "songs")], None)))));
    assert!(d.receive(&ctx));
    assert!(matches!(d.state, State::Loading(_)), "a stale reply landed");
    let first = page(&[("a", "songs")], Some("/v1/next"));
    answer(&mut d, &ctx, Ok(Reply::Detail(found(first))));
    assert_eq!(
        tick(&mut d, t0, true, &album),
        Some(Call::Next("/v1/next".into()))
    );
    assert_eq!(tick(&mut d, t0, true, &album), None);
    answer(&mut d, &ctx, Ok(Reply::Page(page(&[("b", "songs")], None))));
    assert_eq!(tick(&mut d, t0, true, &album), None);
    assert_eq!(d.status()["trackIds"], json!(["a", "b"]));
    assert_eq!(d.status()["complete"], true);
}

#[test]
fn a_silent_or_failed_fetch_ends_in_an_error_not_a_spinner() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    let playlist = Route::Playlist("pl.x".into());
    assert_eq!(
        tick(&mut d, t0, true, &playlist),
        Some(Call::Playlist("pl.x".into()))
    );
    d.receive(&ctx);
    tick(
        &mut d,
        t0 + super::DEADLINE + Duration::from_millis(1),
        true,
        &playlist,
    );
    assert_eq!(d.status()["state"], "error");
    tick(&mut d, t0, true, &Route::Album("2".into()));
    answer(&mut d, &ctx, Err(Error::SignInNeeded));
    assert_eq!(d.status()["state"], "error");
}

#[test]
fn signed_out_nothing_is_fetched_and_an_open_album_drops_its_tracks_and_art() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    let album = Route::Album("1".into());
    assert_eq!(tick(&mut d, t0, false, &album), None);
    assert_eq!(d.status()["state"], "idle");
    tick(&mut d, t0, true, &album);
    answer(
        &mut d,
        &ctx,
        Ok(Reply::Detail(found(page(&[("a", "songs")], None)))),
    );
    let State::Loaded(model) = &d.state else {
        panic!("not loaded")
    };
    d.art.want(&ctx, &model.item.clone());
    assert_eq!(d.art.wanted(), 1);
    assert_eq!(tick(&mut d, t0, false, &album), None);
    assert_eq!(
        (d.status()["tracks"].clone(), d.art.wanted()),
        (json!(0), 0)
    );
}

#[test]
fn a_fresh_album_replaces_the_stale_one_and_only_the_first_fetch_revalidates() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    let album = Route::Album("1".into());
    let mut first = d.tick(t0, true, &album, || {}).unwrap();
    let refresh = first.refresh.take().expect("the album fetch revalidates");
    let stale = page(&[("a", "songs")], Some("/v1/next"));
    (first.done)(Ok(Reply::Detail(found(stale))));
    d.receive(&ctx);
    let next = d.tick(t0, true, &album, || {}).unwrap();
    assert!(next.refresh.is_none(), "a paging fetch is always live");
    refresh(Ok(Reply::Detail(found(page(
        &[("a", "songs"), ("b", "songs")],
        None,
    )))));
    d.receive(&ctx);
    assert_eq!(d.status()["trackIds"], json!(["a", "b"]));
    assert_eq!(d.status()["complete"], true);
    (next.done)(Ok(Reply::Page(page(&[("z", "songs")], None))));
    d.receive(&ctx);
    assert_eq!(
        d.status()["trackIds"],
        json!(["a", "b"]),
        "the old paging reply is dropped"
    );
}

#[test]
fn a_403_on_revalidating_an_album_reopens_it_live() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    let album = Route::Album("1".into());
    let mut first = d.tick(t0, true, &album, || {}).unwrap();
    let refresh = first.refresh.take().unwrap();
    (first.done)(Ok(Reply::Detail(found(page(&[("a", "songs")], None)))));
    d.receive(&ctx);
    refresh(Err(Error::SignInNeeded));
    d.receive(&ctx);
    let again = d
        .tick(t0, true, &album, || {})
        .expect("it asks again, live");
    assert_eq!(again.call, Call::Album("1".into()));
    assert!(matches!(d.state, State::Loading(_)));
}

#[test]
fn a_narrow_header_stacks_the_cover_above_the_title_and_wraps_the_buttons_inside_the_window() {
    let (ctx, mut d, t0) = (egui::Context::default(), Detail::new(None), Instant::now());
    ctx.enable_accesskit();
    tick(&mut d, t0, true, &Route::Album("1".into()));
    answer(
        &mut d,
        &ctx,
        Ok(Reply::Detail(found(page(&[("a", "songs")], None)))),
    );
    for width in [360.0, 400.0, 459.0, 700.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0));
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| super::show(ui, &mut d, &mut Vec::new()));
            out.textures_delta.clear();
            nodes = out.platform_output.accesskit_update.expect("a tree").nodes;
        }
        let at = |label: &str| {
            let named = |n: &egui::accesskit::Node| n.label().or(n.value()) == Some(label);
            let node = nodes.iter().find(|(_, n)| named(n));
            node.and_then(|(_, n)| n.bounds()).expect(label)
        };
        for label in ["Play all", "Shuffle", "Play songs a"] {
            let rect = at(label);
            assert!(
                rect.x0 >= 0.0 && rect.x1 <= f64::from(width),
                "{width}: {label} {rect:?}"
            );
        }
        let title = at("albums 1");
        let stacked = title.x0 < 40.0;
        assert_eq!(stacked, width < 460.0, "{width}: title at {title:?}");
        assert!(
            title.x1 - title.x0 > 60.0,
            "{width}: the title is squeezed to {title:?}"
        );
    }
}
