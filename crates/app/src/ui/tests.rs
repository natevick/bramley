use std::sync::mpsc::channel;

use egui::{Event, Key, Modifiers, RawInput};

use super::content::search_id;
use super::nav::{BACK_LIMIT, Nav, Route, Shell};
use super::shortcuts;
use crate::playback::Action;

fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}

fn press(k: Key, modifiers: Modifiers) -> RawInput {
    RawInput {
        events: vec![key(k, true, modifiers)],
        ..RawInput::default()
    }
}

fn tap(k: Key) -> RawInput {
    let events = vec![
        key(k, false, Modifiers::NONE),
        key(k, true, Modifiers::NONE),
    ];
    RawInput {
        events,
        ..RawInput::default()
    }
}

#[test]
fn navigation_pushes_the_route_left_and_back_returns_to_it() {
    let mut shell = Shell::default();
    assert_eq!(shell.route, Route::Home);
    assert!(!shell.can_go_back());
    shell.apply(Nav::Go(Route::Albums));
    shell.apply(Nav::Go(Route::Albums));
    shell.apply(Nav::Go(Route::Search));
    assert_eq!(shell.route, Route::Search);
    shell.apply(Nav::Back);
    assert_eq!(shell.route, Route::Albums);
    shell.apply(Nav::Back);
    assert_eq!(shell.route, Route::Home);
    shell.apply(Nav::Back);
    assert_eq!(shell.route, Route::Home);
    assert!(!shell.can_go_back());
}

#[test]
fn the_back_stack_keeps_only_the_latest_routes() {
    let mut shell = Shell::default();
    for i in 0..BACK_LIMIT * 2 {
        shell.apply(Nav::Go([Route::Albums, Route::Songs][i % 2].clone()));
    }
    let mut backs = 0;
    while shell.can_go_back() && backs <= BACK_LIMIT {
        shell.apply(Nav::Back);
        backs += 1;
    }
    assert_eq!(backs, BACK_LIMIT);
    assert_eq!(shell.route, Route::Songs);
}

#[test]
fn the_queue_toggles_and_focus_search_opens_search_wanting_focus() {
    let mut shell = Shell::default();
    shell.apply(Nav::ToggleQueue);
    assert!(shell.queue_open);
    shell.apply(Nav::ToggleQueue);
    assert!(!shell.queue_open);
    shell.apply(Nav::FocusSearch);
    assert_eq!((&shell.route, shell.focus_search), (&Route::Search, true));
    shell.apply(Nav::Back);
    assert_eq!(shell.route, Route::Home);
}

fn keyed(ctx: &egui::Context, input: RawInput, toggle: Option<Action>) -> (Vec<Action>, Vec<Nav>) {
    let (mut actions, mut navs, mut text) = (Vec::new(), Vec::new(), String::new());
    let mut out = ctx.run_ui(input, |ui| {
        shortcuts(ui.ctx(), toggle.clone(), &mut actions, &mut navs);
        ui.add(egui::TextEdit::singleline(&mut text).id(search_id()));
    });
    out.textures_delta.clear();
    (actions, navs)
}

#[test]
fn space_toggles_play_only_when_no_text_field_has_focus() {
    let ctx = egui::Context::default();
    let space = || tap(Key::Space);
    assert_eq!(keyed(&ctx, space(), Some(Action::Pause)).0, [Action::Pause]);
    assert_eq!(keyed(&ctx, space(), None).0, []);
    ctx.memory_mut(|m| m.request_focus(search_id()));
    keyed(&ctx, RawInput::default(), None);
    assert!(ctx.text_edit_focused());
    assert_eq!(keyed(&ctx, space(), Some(Action::Play)).0, []);
}

#[test]
fn holding_space_toggles_once_and_swallows_the_repeats() {
    let ctx = egui::Context::default();
    let held = || press(Key::Space, Modifiers::NONE);
    assert_eq!(keyed(&ctx, held(), Some(Action::Pause)).0, [Action::Pause]);
    assert_eq!(keyed(&ctx, held(), Some(Action::Play)).0, []);
    assert!(!ctx.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { .. }))));
    assert_eq!(
        keyed(&ctx, tap(Key::Space), Some(Action::Play)).0,
        [Action::Play]
    );
}

#[test]
fn command_l_focuses_search_even_while_typing() {
    let ctx = egui::Context::default();
    let ctrl_l = || press(Key::L, Modifiers::COMMAND);
    assert_eq!(keyed(&ctx, ctrl_l(), None).1, [Nav::FocusSearch]);
    ctx.memory_mut(|m| m.request_focus(search_id()));
    keyed(&ctx, RawInput::default(), None);
    assert_eq!(keyed(&ctx, ctrl_l(), None).1, [Nav::FocusSearch]);
    assert_eq!(keyed(&ctx, press(Key::L, Modifiers::NONE), None).1, []);
}

fn app(ctx: &egui::Context, signed_out: bool) -> crate::app::App {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/no-config");
    let spawn = crate::tests::spawning(|| unreachable!("no player may start in these tests"));
    let user = (crate::tests::mock_user(&dir), signed_out);
    crate::app::App::new(
        ctx,
        crate::Args::default(),
        spawn,
        user,
        (channel().1, true),
        None,
    )
}

fn frame(ctx: &egui::Context, app: &mut crate::app::App, input: RawInput) -> egui::FullOutput {
    let mut out = ctx.run_ui(input, |ui| app.draw(ui));
    out.textures_delta.clear();
    out
}

#[test]
fn the_app_applies_command_l_after_the_frame_and_focuses_the_field_next_frame() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx, true);
    frame(&ctx, &mut app, tap(Key::Space));
    frame(&ctx, &mut app, press(Key::L, Modifiers::COMMAND));
    assert_eq!(app.status_json()["route"], "Search");
    frame(&ctx, &mut app, RawInput::default());
    assert_eq!(ctx.memory(|m| m.focused()), Some(search_id()));
    ctx.memory_mut(|m| m.surrender_focus(search_id()));
    frame(&ctx, &mut app, RawInput::default());
    frame(&ctx, &mut app, RawInput::default());
    assert_eq!(ctx.memory(|m| m.focused()), None);
    assert_eq!(app.status_json()["engineStarted"], false);
}

#[test]
fn nav_items_and_player_controls_carry_accessible_labels() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = app(&ctx, true);
    let update = frame(&ctx, &mut app, RawInput::default())
        .platform_output
        .accesskit_update;
    let nodes = update.expect("an accesskit tree").nodes;
    let labels: Vec<_> = nodes.iter().filter_map(|(_, n)| n.label()).collect();
    let wanted = "Home,Search,Playlists,Albums,Songs,Back,Queue,Previous track,Play,Next track";
    for want in wanted.split(',').chain(["Position"]) {
        assert!(labels.contains(&want), "no {want:?} in {labels:?}");
    }
    frame(&ctx, &mut app, press(Key::L, Modifiers::COMMAND));
    let update = frame(&ctx, &mut app, RawInput::default()).platform_output;
    let nodes = update.accesskit_update.expect("an accesskit tree").nodes;
    let label = |id| {
        nodes
            .iter()
            .find(|(n, _)| *n == id)
            .and_then(|(_, n)| n.value())
    };
    let field = nodes
        .iter()
        .find(|(_, n)| n.role() == egui::accesskit::Role::TextInput);
    let names: Vec<_> = field
        .expect("a search field")
        .1
        .labelled_by()
        .iter()
        .map(|&id| label(id))
        .collect();
    assert_eq!(names, [Some("Search")]);
}

#[test]
fn typing_then_enter_reaches_the_api_worker_only_when_signed_in() {
    for signed_out in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, (api, jobs)) = (app(&ctx, signed_out), channel());
        app.api = Some(api);
        frame(&ctx, &mut app, press(Key::L, Modifiers::COMMAND));
        frame(&ctx, &mut app, RawInput::default());
        let typed = vec![
            Event::Text("queen".into()),
            key(Key::Enter, true, Modifiers::NONE),
        ];
        frame(
            &ctx,
            &mut app,
            RawInput {
                events: typed,
                ..RawInput::default()
            },
        );
        app.tick(&ctx);
        let calls: Vec<_> = jobs.try_iter().map(|job| job.call).collect();
        let want = (!signed_out).then(|| bramley_api::Call::Search("queen".into()));
        assert_eq!(calls, Vec::from_iter(want));
        assert_eq!(app.status_json()["engineStarted"], false);
    }
}

mod search {
    use std::time::{Duration, Instant};

    use bramley_api::{Call, Error, Item, Job, Page, Reply, SearchResults};
    use egui::{Color32, ColorImage, RawInput};
    use serde_json::json;

    use super::super::nav::{Nav, Route, Shell};
    use super::super::search::{Search, State, art_key, cause, clicked, sections, show};
    use crate::playback::Action;

    const LOCAL: &str = "http://127.0.0.1:9";

    fn item(id: &str, kind: &str) -> Item {
        let art = json!({ "url": format!("{LOCAL}/{id}/{{w}}x{{h}}bb.jpg") });
        let attributes =
            json!({ "name": format!("{kind} {id}"), "artistName": "Queen", "artwork": art });
        serde_json::from_value(json!({ "id": id, "type": kind, "attributes": attributes })).unwrap()
    }

    fn page(kind: &str, ids: &[&str]) -> Page {
        let items = ids.iter().map(|id| item(id, kind)).collect();
        Page {
            items,
            ..Page::default()
        }
    }

    fn found(songs: &[&str]) -> SearchResults {
        SearchResults {
            songs: page("songs", songs),
            ..SearchResults::default()
        }
    }

    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }

    fn ask(search: &mut Search, term: &str, at: Instant) -> Job {
        search.enter(term, at);
        search.tick(at, true, || {}).expect("a search job")
    }

    fn answer(ctx: &egui::Context, search: &mut Search, term: &str, songs: &[&str]) {
        (ask(search, term, Instant::now()).done)(Ok(Reply::Search(found(songs))));
        search.receive(ctx);
    }

    fn draw(ctx: &egui::Context, search: &mut Search) -> egui::FullOutput {
        let (mut shell, mut actions, mut navs) = (Shell::default(), Vec::new(), Vec::new());
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            show(
                ui,
                (&mut shell, search),
                egui::Id::NULL,
                (&mut actions, &mut navs),
            );
        });
        out.textures_delta.clear();
        out
    }

    #[test]
    fn three_keystrokes_within_300ms_send_one_trimmed_search_after_the_debounce() {
        let (mut search, t0) = (Search::new(None), Instant::now());
        let mut jobs = Vec::new();
        for (n, typed) in [(0, "b"), (100, "bo"), (200, " boh ")] {
            search.edited(typed, ms(t0, n));
            jobs.extend(search.tick(ms(t0, n), true, || {}));
        }
        jobs.extend(search.tick(ms(t0, 449), true, || {}));
        assert!(jobs.is_empty());
        assert_eq!(search.wait(ms(t0, 300)), Some(Duration::from_millis(150)));
        jobs.extend((450..460).filter_map(|n| search.tick(ms(t0, n), true, || {})));
        let calls: Vec<_> = jobs.iter().map(|job| &job.call).collect();
        assert_eq!(calls, [&Call::Search("boh".into())]);
        assert!(jobs[0].deadline > ms(t0, 450));
    }

    #[test]
    fn a_superseded_reply_is_never_shown_whichever_order_they_land() {
        let ctx = egui::Context::default();
        let (mut search, t0) = (Search::new(None), Instant::now());
        let old = ask(&mut search, "queen", t0);
        let new = ask(&mut search, "queen live", ms(t0, 10));
        (old.done)(Ok(Reply::Search(found(&["1"]))));
        search.receive(&ctx);
        assert!(matches!(search.state, State::Loading(_)));
        (new.done)(Ok(Reply::Search(found(&["2"]))));
        let stale = ask(&mut search, "x", ms(t0, 20));
        let fresh = ask(&mut search, "xy", ms(t0, 30));
        (fresh.done)(Ok(Reply::Search(found(&["3"]))));
        (stale.done)(Ok(Reply::Search(found(&["4"]))));
        search.receive(&ctx);
        assert_eq!(search.state, State::Results(found(&["3"])));
        assert_eq!(search.status()["results"], 1);
    }

    #[test]
    fn results_group_by_kind_in_order_and_nothing_found_is_empty() {
        let ctx = egui::Context::default();
        let mut r = found(&["1", "2"]);
        (r.albums, r.artists) = (page("albums", &["3"]), page("artists", &["4"]));
        let titles: Vec<_> = sections(&r).iter().map(|(t, s)| (*t, s.len())).collect();
        assert_eq!(titles, [("Songs", 2), ("Albums", 1), ("Artists", 1)]);
        r.playlists = page("playlists", &["pl.5"]);
        assert_eq!(sections(&r).last().map(|(t, _)| *t), Some("Playlists"));
        let mut search = Search::new(None);
        answer(&ctx, &mut search, "zzz", &[]);
        assert_eq!(search.state, State::Empty);
    }

    #[test]
    fn a_song_click_plays_it_and_albums_and_playlists_navigate() {
        let (mut actions, mut navs) = (Vec::new(), Vec::new());
        let clicks = [
            ("1", "songs"),
            ("2", "albums"),
            ("pl.3", "playlists"),
            ("4", "artists"),
        ];
        for (id, kind) in clicks.into_iter().chain([("../5", "songs")]) {
            clicked(&item(id, kind), &mut actions, &mut navs);
        }
        assert_eq!(actions, [Action::PlaySong("1".into())]);
        let routes = [Route::Album("2".into()), Route::Playlist("pl.3".into())];
        assert_eq!(navs, routes.map(Nav::Go));
    }

    #[test]
    fn art_stays_while_shown_and_is_dropped_when_results_are_replaced() {
        let ctx = egui::Context::default();
        let mut search = Search::new(Some(LOCAL.into()));
        answer(&ctx, &mut search, "a", &["1"]);
        draw(&ctx, &mut search);
        let (art, set) = (search.art.tx.clone(), search.art.set());
        let image = || ColorImage::filled([2, 2], Color32::RED);
        art.send((set, "Songs/1".into(), image())).unwrap();
        search.receive(&ctx);
        draw(&ctx, &mut search);
        assert_eq!(search.status()["art"], 1);
        assert_ne!(
            art_key(&item("1", "albums")),
            art_key(&item("1", "artists"))
        );
        let next = ask(&mut search, "b", Instant::now());
        art.send((set, "Songs/1".into(), image())).unwrap();
        (next.done)(Ok(Reply::Search(found(&["1"]))));
        search.receive(&ctx);
        assert_eq!(search.status()["art"], 0);
    }

    #[test]
    fn signed_out_never_searches_and_clears_what_was_shown() {
        let ctx = egui::Context::default();
        let (mut search, t0) = (Search::new(None), Instant::now());
        answer(&ctx, &mut search, "a", &["1"]);
        search.enter("b", t0);
        assert!(search.tick(ms(t0, 1), false, || {}).is_none());
        assert_eq!(search.state, State::Idle);
        assert!(search.tick(ms(t0, 2), true, || {}).is_some());
    }

    #[test]
    fn a_search_that_never_answers_times_out_instead_of_loading_forever() {
        let (mut search, t0) = (Search::new(None), Instant::now());
        let job = ask(&mut search, "a", t0);
        search.tick(job.deadline, true, || {});
        assert!(matches!(search.state, State::Loading(_)));
        search.tick(job.deadline + Duration::from_millis(1), true, || {});
        assert!(matches!(search.state, State::Failed(_)));
    }

    #[test]
    fn a_failed_search_shows_neither_the_term_nor_the_wire_detail() {
        let wire = Error::Network("GET https://x/search?term=bohemian: refused".into());
        assert_eq!(cause(&wire), "Apple Music could not be reached");
        assert!(!cause(&Error::Http(500, "term bohemian".into())).contains("bohemian"));
    }

    #[test]
    fn result_rows_are_labelled_buttons() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut search = Search::new(None);
        let mut r = found(&["1"]);
        r.albums = page("albums", &["2"]);
        search.state = State::Results(r);
        let nodes = draw(&ctx, &mut search).platform_output.accesskit_update;
        let nodes = nodes.expect("a tree").nodes;
        let button = |n: &egui::accesskit::Node| n.role() == egui::accesskit::Role::Button;
        let mut labels: Vec<_> = nodes
            .iter()
            .filter(|(_, n)| button(n))
            .filter_map(|(_, n)| n.label())
            .collect();
        labels.sort();
        assert_eq!(
            labels,
            ["Open album albums 2 · Queen", "Play songs 1 · Queen"]
        );
    }
}
