use std::time::{Duration, Instant};

use bramley_api::{Call, Detail as Found, Error, Item, Page, Reply};
use bramley_engine::Queue;
use serde_json::json;

use super::{Detail, Model, State, container, play_from};
use crate::playback::Action;
use crate::ui::nav::Route;

fn item(id: &str, kind: &str, ms: u64) -> Item {
    let attributes = json!({ "name": format!("{kind} {id}"), "durationInMillis": ms,
        "artwork": { "url": "http://127.0.0.1:9/a/{w}x{h}bb.jpg" } });
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
fn the_model_keeps_songs_with_their_container_place_and_sums_only_when_complete() {
    let songs = [("a", "songs"), ("v", "music-videos"), ("b", "songs")];
    let mut model = Model::new(found(page(&songs, Some("/next"))));
    assert_eq!((model.tracks.len(), &model.at), (2, &vec![0, 2]));
    assert_eq!(model.summary(), "2+ songs");
    model.add(page(&[("c", "songs")], None));
    assert_eq!(model.at, [0, 2, 3]);
    assert_eq!(model.summary(), "3 songs, 3 min");
    model.broken = true;
    assert_eq!(model.summary(), "3+ songs");
}

#[test]
fn a_click_plays_the_container_from_that_songs_place_in_it() {
    let model = Model::new(found(page(&[("v", "music-videos"), ("a", "songs")], None)));
    let album = container(&Route::Album("1".into())).unwrap();
    assert_eq!(album, Queue::Album("1".into()));
    assert_eq!(
        play_from(&album, &model, 0),
        Some(Action::PlayIn(album.clone(), 1))
    );
    assert_eq!(play_from(&album, &model, 1), None);
    let catalog = Route::Playlist("pl.x".into());
    assert_eq!(container(&catalog), Some(Queue::Playlist("pl.x".into())));
    assert_eq!(container(&Route::Playlist("p.mine".into())), None);
    assert_eq!(container(&Route::Home), None);
}

fn tick(d: &mut Detail, now: Instant, signed_in: bool, route: &Route) -> Option<Call> {
    d.tick(now, signed_in, route, || {}).map(|job| job.call)
}

fn answer(d: &mut Detail, ctx: &egui::Context, reply: bramley_api::Result<Reply>) {
    d.tx.send((d.seq, reply)).unwrap();
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
