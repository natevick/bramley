use std::sync::mpsc::channel;

use egui::{Event, Key, Modifiers, RawInput};

use super::content::search_id;
use super::keys::BINDINGS;
use super::nav::{BACK_LIMIT, Edge, Nav, Route, Shell};
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

pub fn hooked(ctx: &egui::Context, k: Key) -> RawInput {
    let mut input = press(k, Modifiers::NONE);
    super::keys::take(ctx, &mut input);
    input
}

pub fn focus_label(tree: &egui::accesskit::TreeUpdate) -> Option<String> {
    let (_, node) = tree.nodes.iter().find(|(id, _)| *id == tree.focus)?;
    Some(node.label()?.to_owned())
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
fn artist_routes_parse_and_their_title_never_names_the_artist() {
    assert_eq!(Route::parse("artists"), Some(Route::Artists));
    let artist = Route::parse("artist:r.1").expect("an artist route");
    assert_eq!(
        (&artist, artist.title()),
        (&Route::Artist("r.1".into()), "Artist")
    );
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
fn a_scroll_waits_in_the_shell_until_a_route_change_or_back_drops_it() {
    let playlist = |id: &str| Route::Playlist(id.into()).lists();
    assert_eq!((playlist("p.1"), playlist("pl.u-1")), (true, false));
    let album = |id: &str| Route::Album(id.into()).lists();
    assert_eq!((album("l.1"), album("1440")), (true, false));
    let mut shell = Shell::default();
    for leave in [Nav::Go(Route::Songs), Nav::Back] {
        shell.apply(Nav::Scroll(Edge::End));
        assert_eq!(shell.scroll, Some(Edge::End));
        shell.apply(leave);
        assert_eq!(shell.scroll, None);
    }
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
fn the_artists_list_neither_swallows_space_nor_command_l_nor_types_for_them() {
    let ctx = egui::Context::default();
    let mut lib = super::library::Library::default();
    let events = vec![
        key(Key::Space, true, Modifiers::NONE),
        Event::Text(" ".into()),
        key(Key::L, true, Modifiers::COMMAND),
    ];
    let (mut actions, mut navs) = (Vec::new(), Vec::new());
    let mut out = ctx.run_ui(
        RawInput {
            events,
            ..RawInput::default()
        },
        |ui| {
            let route = Route::Artists;
            let emit = (&mut Vec::new(), &mut Vec::new());
            super::library::show(ui, &mut lib, (&route, &mut None), emit);
            shortcuts(ui.ctx(), Some(Action::Pause), &mut actions, &mut navs);
        },
    );
    out.textures_delta.clear();
    assert_eq!(
        (actions, navs),
        (vec![Action::Pause], vec![Nav::FocusSearch])
    );
    assert!(lib.jump.is_none());
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

fn frame(ctx: &egui::Context, app: &mut crate::app::App, mut input: RawInput) -> egui::FullOutput {
    eframe::App::raw_input_hook(app, ctx, &mut input);
    unhooked(ctx, app, input)
}

fn unhooked(ctx: &egui::Context, app: &mut crate::app::App, input: RawInput) -> egui::FullOutput {
    let mut out = ctx.run_ui(input, |ui| app.draw(ui));
    out.textures_delta.clear();
    out
}

#[test]
#[cfg(target_os = "macos")]
fn the_player_bar_has_no_volume_slider_because_the_system_player_has_no_volume() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = app(&ctx, true);
    let out = frame(&ctx, &mut app, RawInput::default());
    let tree = out.platform_output.accesskit_update.expect("a tree");
    let labels: Vec<_> = tree.nodes.iter().filter_map(|(_, n)| n.label()).collect();
    assert!(labels.contains(&"Position"));
    assert!(!labels.contains(&"Volume"));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn a_volume_set_from_the_cli_shows_on_the_slider_in_the_app() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = app(&ctx, true);
    app.apply(Action::Volume(0.4));
    let out = frame(&ctx, &mut app, RawInput::default());
    let tree = out.platform_output.accesskit_update.expect("a tree");
    let nodes: Nodes = tree
        .nodes
        .into_iter()
        .filter_map(|(_, n)| Some((n.label()?.to_owned(), n)))
        .collect();
    assert_eq!(volume_of(&nodes), Some(40.0));
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
    let wanted =
        "Home,Search,Artists,Playlists,Albums,Songs,Back,Queue,Previous track,Play,Next track";
    for want in wanted.split(',').chain(["Position"]) {
        assert!(labels.contains(&want), "no {want:?} in {labels:?}");
    }
    let top = |name| {
        let node = nodes.iter().find(|(_, n)| n.label() == Some(name));
        node.and_then(|(_, n)| n.bounds()).expect("on screen").y0
    };
    assert!(
        top("Artists") < top("Playlists"),
        "Artists heads Your Library"
    );
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
        let searched = |call: &bramley_api::Call| matches!(call, bramley_api::Call::Search(_));
        let calls: Vec<_> = jobs
            .try_iter()
            .map(|job| job.call)
            .filter(searched)
            .collect();
        let want = (!signed_out).then(|| bramley_api::Call::Search("queen".into()));
        assert_eq!(calls, Vec::from_iter(want));
        assert_eq!(app.status_json()["engineStarted"], false);
    }
}

#[test]
fn an_open_album_reaches_the_api_worker_only_when_signed_in() {
    for signed_out in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, (api, jobs)) = (app(&ctx, signed_out), channel());
        app.api = Some(api);
        app.go(Route::Album("123".into()));
        app.tick(&ctx);
        frame(&ctx, &mut app, RawInput::default());
        let calls: Vec<_> = jobs.try_iter().map(|job| job.call).collect();
        let want = (!signed_out).then(|| bramley_api::Call::Album("123".into()));
        assert_eq!(calls, Vec::from_iter(want));
        assert_eq!(app.status_json()["engineStarted"], false);
    }
}

#[test]
fn typing_while_signed_out_never_spins_the_repaint_loop() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx, true);
    frame(&ctx, &mut app, press(Key::L, Modifiers::COMMAND));
    let typed = vec![Event::Text("a".into())];
    frame(
        &ctx,
        &mut app,
        RawInput {
            events: typed,
            ..RawInput::default()
        },
    );
    std::thread::sleep(super::search::DEBOUNCE + std::time::Duration::from_millis(50));
    let delays: Vec<_> = (0..4)
        .map(|_| {
            let mut out = ctx.run_ui(RawInput::default(), |ui| {
                app.tick(ui.ctx());
                app.draw(ui);
            });
            out.textures_delta.clear();
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        })
        .collect();
    let floor = std::time::Duration::from_millis(100);
    assert!(delays[1..].iter().all(|d| *d >= floor), "{delays:?}");
}

mod search {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use bramley_api::{Call, Error, Item, Job, Page, Reply, SearchResults};
    use egui::{Color32, ColorImage, RawInput};
    use serde_json::json;

    use super::super::nav::{Nav, Route, Shell};
    use super::super::search::{
        Search, State, art_key, cause, clicked, results_list, sections, show,
    };
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
        assert!(jobs[0].deadline > ms(t0, 450) && jobs[0].supersedable);
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
    fn results_group_artists_songs_albums_playlists_in_order() {
        let ctx = egui::Context::default();
        let mut r = found(&["1", "2"]);
        (r.albums, r.artists) = (page("albums", &["3"]), page("artists", &["4"]));
        r.playlists = page("playlists", &["pl.5"]);
        let titles: Vec<_> = sections(&r).iter().map(|(t, s)| (*t, s.len())).collect();
        assert_eq!(
            titles,
            [
                ("Artists", 1),
                ("Songs", 2),
                ("Albums", 1),
                ("Playlists", 1)
            ]
        );
        let mut search = Search::new(None);
        (ask(&mut search, "a", Instant::now()).done)(Ok(Reply::Search(r)));
        search.receive(&ctx);
        let keys = ["songs", "albums", "artists", "playlists", "results"];
        let counts = keys.map(|k| search.status()[k].clone());
        assert_eq!(counts, [2, 1, 1, 1, 5].map(serde_json::Value::from));
        let only_artists = SearchResults {
            artists: page("artists", &["4"]),
            ..SearchResults::default()
        };
        (ask(&mut search, "b", Instant::now()).done)(Ok(Reply::Search(only_artists)));
        search.receive(&ctx);
        assert!(matches!(search.state, State::Results(_)));
    }

    #[test]
    fn a_song_click_plays_it_and_albums_playlists_and_artists_navigate() {
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
        let routes = [
            Route::Album("2".into()),
            Route::Playlist("pl.3".into()),
            Route::Artist("4".into()),
        ];
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

    fn gated_art_server() -> (String, Arc<Mutex<Vec<String>>>, Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (log, (open, gate)) = (Arc::new(Mutex::new(Vec::new())), channel());
        let seen_by_server = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
                let first = seen_by_server.lock().unwrap().is_empty();
                seen_by_server.lock().unwrap().push(path);
                if first {
                    let _ = gate.recv();
                }
            }
        });
        (base, log, open)
    }

    fn results_at(base: &str, n: usize) -> SearchResults {
        let art = |i: usize| json!({ "url": format!("{base}/{i}/{{w}}x{{h}}bb.jpg") });
        let song = |i: usize| json!({ "id": i.to_string(), "type": "songs", "attributes": { "name": "s", "artwork": art(i) } });
        let data: Vec<_> = (0..n).map(song).collect();
        serde_json::from_value(json!({ "songs": { "data": data } })).unwrap()
    }

    fn draw_at(ctx: &egui::Context, search: &mut Search, height: f32) {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, height));
        let input = RawInput {
            screen_rect: Some(rect),
            ..RawInput::default()
        };
        let (mut shell, mut actions, mut navs) = (Shell::default(), Vec::new(), Vec::new());
        let mut out = ctx.run_ui(input, |ui| {
            show(
                ui,
                (&mut shell, search),
                egui::Id::NULL,
                (&mut actions, &mut navs),
            );
        });
        out.textures_delta.clear();
    }

    #[test]
    fn art_is_fetched_only_for_visible_rows_of_the_current_results() {
        let (base, seen, open) = gated_art_server();
        let ctx = egui::Context::default();
        let mut search = Search::new(Some(base.clone()));
        (ask(&mut search, "a", Instant::now()).done)(Ok(Reply::Search(results_at(&base, 30))));
        search.receive(&ctx);
        draw_at(&ctx, &mut search, 300.0);
        let visible = search.art.wanted();
        assert!((1..30).contains(&visible), "{visible}");
        draw_at(&ctx, &mut search, 4000.0);
        assert_eq!(search.art.wanted(), 30);
        let replaced = results_at(&format!("{base}/new"), 1);
        (ask(&mut search, "b", Instant::now()).done)(Ok(Reply::Search(replaced)));
        search.receive(&ctx);
        draw_at(&ctx, &mut search, 300.0);
        open.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let done = || seen.lock().unwrap().iter().any(|p| p.starts_with("/new/"));
        while !done() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut fetched = seen.lock().unwrap().clone();
        fetched.dedup();
        assert_eq!(fetched.len(), 2, "{fetched:?}");
    }

    #[cfg(feature = "dev")]
    #[test]
    fn the_dev_search_hook_waits_for_sign_in() {
        let (mut search, t0) = (Search::new(None), Instant::now());
        search.typed_once_signed_in = Some("queen".into());
        assert!(search.tick(t0, false, || {}).is_none());
        let job = search
            .tick(ms(t0, 1), true, || {})
            .expect("a search once signed in");
        assert_eq!(job.call, Call::Search("queen".into()));
    }

    #[test]
    fn signed_out_never_searches_and_clears_what_was_shown() {
        let ctx = egui::Context::default();
        let (mut search, t0) = (Search::new(None), Instant::now());
        answer(&ctx, &mut search, "a", &["1"]);
        search.enter("b", t0);
        assert!(search.tick(ms(t0, 1), false, || {}).is_none());
        assert_eq!(
            (&search.state, search.wait(ms(t0, 1))),
            (&State::Idle, None)
        );
        assert!(search.tick(ms(t0, 2), true, || {}).is_none());
    }

    #[test]
    fn a_search_that_never_answers_times_out_instead_of_loading_forever() {
        let (mut search, t0) = (Search::new(None), Instant::now());
        let job = ask(&mut search, "a", t0);
        search.tick(job.deadline, true, || {});
        assert!(matches!(search.state, State::Loading(_)));
        search.tick(job.deadline + Duration::from_millis(1), true, || {});
        (job.done)(Ok(Reply::Search(found(&["1"]))));
        search.receive(&egui::Context::default());
        assert!(matches!(search.state, State::Failed(..)));
        assert_eq!(search.status()["error"], "Timeout");
    }

    #[test]
    fn a_cli_search_never_outlives_the_deadline_its_client_was_given() {
        let (mut search, t0) = (Search::new(None), Instant::now());
        let limit = ms(t0, 3_000);
        search.enter_by("queen", t0, limit);
        let job = search.tick(t0, true, || {}).expect("a search job");
        search.tick(ms(t0, 3_001), true, || {});
        let loading = matches!(search.state, State::Loading(_));
        search.enter("queen", t0);
        let typed = search.tick(t0, true, || {}).expect("a search job");
        assert_eq!(
            (job.deadline, loading, typed.deadline),
            (limit, true, t0 + crate::ui::search::DEADLINE)
        );
    }

    #[test]
    fn a_failed_search_shows_neither_the_term_nor_the_wire_detail() {
        let wire = Error::Network("GET https://x/search?term=bohemian: refused".into());
        assert_eq!(cause(&wire), "Apple Music could not be reached");
        assert!(!cause(&Error::Http(500, "term bohemian".into())).contains("bohemian"));
        let ctx = egui::Context::default();
        let mut search = Search::new(None);
        let keyring = "the system keyring did not answer; unlock it";
        (ask(&mut search, "bohemian", Instant::now()).done)(Err(Error::Config(keyring.into())));
        search.receive(&ctx);
        assert_eq!(search.state, State::Failed(keyring.into(), "Config".into()));
        assert!(!search.status().to_string().contains("bohemian"));
    }

    fn list_width(name: &str) -> f32 {
        let ctx = egui::Context::default();
        let mut search = Search::new(None);
        let mut r = found(&["1"]);
        r.songs.items[0].name = Some(name.into());
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 400.0));
        let mut width = 0.0;
        for _ in 0..3 {
            let input = RawInput {
                screen_rect: Some(rect),
                ..RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let emit = (&mut Vec::new(), &mut Vec::new());
                results_list(ui, &r, &mut search.art, emit);
                width = ui.min_rect().width();
            });
            out.textures_delta.clear();
        }
        width
    }

    #[test]
    fn the_results_column_keeps_the_full_width_whatever_the_names() {
        let short = list_width("Hi");
        let long = list_width(&"Bohemian ".repeat(6));
        assert_eq!(short, long);
        assert!(short > 450.0, "{short}");
    }

    fn recent_albums() -> Reply {
        let mut page = page("albums", &["10", "11"]);
        page.items.push(item("pl.12", "playlists"));
        Reply::Page(page)
    }

    fn recents(ctx: &egui::Context, search: &mut Search, query: &str) -> Vec<String> {
        ctx.enable_accesskit();
        let mut shell = Shell::default();
        shell.query = query.into();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            let emit = (&mut Vec::new(), &mut Vec::new());
            show(ui, (&mut shell, search), egui::Id::NULL, emit);
        });
        out.textures_delta.clear();
        let nodes = out.platform_output.accesskit_update.expect("a tree").nodes;
        let labels = nodes.iter().filter_map(|(_, n)| n.label());
        labels
            .filter(|l| l.starts_with("Open album"))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn an_empty_search_shows_recently_played_albums_fetched_once_and_hides_them_while_typing() {
        let (ctx, mut search, now) = (egui::Context::default(), Search::new(None), Instant::now());
        assert!(
            search.recent_job(now, false, || {}).is_none(),
            "not on the page yet"
        );
        let job = search
            .recent_job(now, true, || {})
            .expect("the first visit asks");
        assert_eq!(job.call, Call::RecentlyPlayed);
        assert!(
            search.recent_job(now, true, || {}).is_none(),
            "once while in flight"
        );
        (job.done)(Ok(recent_albums()));
        search.receive(&ctx);
        assert!(
            search.recent_job(now, true, || {}).is_none(),
            "once per session"
        );
        assert_eq!(search.status()["recent"], 2);
        let shown = recents(&ctx, &mut search, "");
        assert_eq!(shown, ["Open album albums 10", "Open album albums 11"]);
        assert!(
            recents(&ctx, &mut search, "bea").is_empty(),
            "hidden while a query is typed"
        );
        assert!(recents(&ctx, &mut search, "  ").len() == 2);
        assert!(
            search.recent_job(now, true, || {}).is_none(),
            "emptying the box asks nothing"
        );
    }

    #[test]
    fn a_failed_recent_fetch_is_not_retried_until_signing_in_again() {
        let (ctx, mut search, now) = (egui::Context::default(), Search::new(None), Instant::now());
        let job = search.recent_job(now, true, || {}).unwrap();
        (job.done)(Err(Error::Network("down".into())));
        search.receive(&ctx);
        assert!(recents(&ctx, &mut search, "").is_empty());
        assert!(search.recent_job(now, true, || {}).is_none());
        search.tick(now, false, || {});
        assert!(search.recent_job(now, true, || {}).is_some());
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

#[test]
fn a_queue_row_jumps_to_its_absolute_index_and_the_current_row_does_nothing() {
    let mut shown = bramley_engine::Snapshot::default();
    let item = |id: &str| bramley_engine::QueueItem {
        id: id.into(),
        title: None,
        duration: None,
    };
    shown.queue = vec![item("a"), item(""), item("c")];
    (shown.queue_position, shown.queue_length) = (Some(40), 43);
    let jump = super::queue_panel::jump;
    assert_eq!(jump(&shown, 2), Some(Action::JumpTo(42, "c".into())));
    assert_eq!(
        (jump(&shown, 0), jump(&shown, 1), jump(&shown, 3)),
        (None, None, None)
    );
    shown.queue_position = None;
    assert_eq!(
        jump(&shown, 0),
        Some(Action::JumpTo(0, "a".into())),
        "nothing is current"
    );
}

type Nodes = Vec<(String, egui::accesskit::Node)>;

fn bar(
    ctx: &egui::Context,
    shown: &bramley_engine::Snapshot,
    started: bool,
    events: Vec<Event>,
) -> (Nodes, Vec<Action>) {
    bar_at(ctx, shown, (started, 100), events)
}

fn bar_at(
    ctx: &egui::Context,
    shown: &bramley_engine::Snapshot,
    (started, volume): (bool, u8),
    events: Vec<Event>,
) -> (Nodes, Vec<Action>) {
    bar_sized(ctx, shown, (started, volume), (1100.0, 80.0), events)
}

fn bar_sized(
    ctx: &egui::Context,
    shown: &bramley_engine::Snapshot,
    (started, volume): (bool, u8),
    (width, height): (f32, f32),
    events: Vec<Event>,
) -> (Nodes, Vec<Action>) {
    ctx.enable_accesskit();
    let mut actions = Vec::new();
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, height),
        )),
        events,
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        let emit = (&mut actions, &mut Vec::new());
        let layout = (false, width < super::COMPACT_PT);
        let margin = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0));
        margin.show(ui, |ui| {
            super::player_bar::show(ui, shown, started, volume, layout, emit)
        });
    });
    out.textures_delta.clear();
    let tree = out.platform_output.accesskit_update.expect("a tree");
    let named = tree
        .nodes
        .into_iter()
        .filter_map(|(_, n)| Some((n.label().or(n.value())?.to_owned(), n)));
    (named.collect(), actions)
}

fn centre(nodes: &Nodes, label: &str) -> egui::Pos2 {
    let node = nodes.iter().find(|(l, _)| l == label);
    let at = node.and_then(|(_, n)| n.bounds()).expect(label);
    egui::pos2(
        ((at.x0 + at.x1) / 2.0) as f32,
        ((at.y0 + at.y1) / 2.0) as f32,
    )
}

fn click_bar(shown: &bramley_engine::Snapshot, started: bool, label: &str) -> Vec<Action> {
    let ctx = egui::Context::default();
    let at = centre(&bar(&ctx, shown, started, vec![]).0, label);
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    bar(
        &ctx,
        shown,
        started,
        vec![Event::PointerMoved(at), button(true)],
    );
    bar(&ctx, shown, started, vec![button(false)]).1
}

#[test]
fn shuffle_and_repeat_sit_at_the_right_end_beside_the_queue() {
    let ctx = egui::Context::default();
    let nodes = bar(&ctx, &bramley_engine::Snapshot::default(), true, vec![]).0;
    let x = |label| centre(&nodes, label).x;
    let order = ["Next track", "Position", "Shuffle", "Repeat off", "Queue"];
    let xs: Vec<f32> = order.iter().map(|l| x(l)).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{order:?} at {xs:?}");
}

fn volume_of(nodes: &Nodes) -> Option<f64> {
    let node = nodes.iter().find(|(l, _)| l == "Volume");
    node.and_then(|(_, n)| n.numeric_value())
}

#[test]
#[cfg(not(target_os = "macos"))]
fn the_volume_slider_sits_between_position_and_shuffle_and_shows_the_current_volume() {
    let ctx = egui::Context::default();
    let shown = bramley_engine::Snapshot::default();
    let nodes = bar_at(&ctx, &shown, (false, 40), vec![]).0;
    let order = ["Position", "Volume", "Shuffle"];
    let xs: Vec<f32> = order.iter().map(|l| centre(&nodes, l).x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{order:?} at {xs:?}");
    assert_eq!(volume_of(&nodes), Some(40.0));
    ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let hover = vec![Event::PointerMoved(centre(&nodes, "Volume"))];
    let tips = |_: u8| bar_at(&ctx, &shown, (false, 40), hover.clone()).0;
    let hovered: Vec<_> = (0..5).map(tips).collect();
    assert!(hovered.iter().any(|n| n.iter().any(|(l, _)| l == "40%")));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn dragging_the_volume_slider_sends_the_value_at_most_ten_times_a_second_and_the_final_one_on_release()
 {
    let ctx = egui::Context::default();
    let shown = bramley_engine::Snapshot::default();
    let slider = |ctx: &egui::Context| {
        let nodes = bar_at(ctx, &shown, (false, 100), vec![]).0;
        let bounds = nodes
            .iter()
            .find(|(l, _)| l == "Volume")
            .unwrap()
            .1
            .bounds();
        bounds.expect("bounds")
    };
    let rect = slider(&ctx);
    let at = |fraction: f64| {
        let x = rect.x0 + (rect.x1 - rect.x0) * fraction;
        egui::pos2(x as f32, ((rect.y0 + rect.y1) / 2.0) as f32)
    };
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let frame = |events| bar_at(&ctx, &shown, (false, 100), events);
    frame(vec![Event::PointerMoved(at(0.2))]);
    let (_, mut sent) = frame(vec![Event::PointerMoved(at(0.2)), button(at(0.2), true)]);
    assert_eq!(sent.len(), 1, "a press sends at once");
    let (nodes, more) = frame(vec![Event::PointerMoved(at(0.7))]);
    assert_eq!(more, [], "a second move inside 100 ms is held back");
    let held = volume_of(&nodes).expect("a value") / 100.0;
    let (_, more) = frame(vec![button(at(0.7), false)]);
    sent.extend(more);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1], Action::Volume(held as f32));
    assert_ne!(sent[0], sent[1]);
    let mut flood = Vec::new();
    frame(vec![Event::PointerMoved(at(0.5)), button(at(0.5), true)]);
    for i in 0..60 {
        let fraction = if i % 2 == 0 { 0.3 } else { 0.8 };
        flood.extend(frame(vec![Event::PointerMoved(at(fraction))]).1);
    }
    assert!(
        flood.len() <= 11,
        "{} commands in about a second",
        flood.len()
    );
}

#[test]
fn the_shuffle_and_repeat_buttons_ask_for_the_next_mode_and_do_nothing_before_a_play() {
    use bramley_engine::Repeat;
    let with = |shuffle, repeat| {
        let mut shown = bramley_engine::Snapshot::default();
        (shown.shuffle, shown.repeat) = (shuffle, repeat);
        shown
    };
    let cases = [
        (
            with(false, Repeat::Off),
            "Shuffle",
            Action::SetShuffle(true),
        ),
        (
            with(true, Repeat::Off),
            "Shuffle",
            Action::SetShuffle(false),
        ),
        (
            with(false, Repeat::Off),
            "Repeat off",
            Action::SetRepeat(Repeat::All),
        ),
        (
            with(false, Repeat::All),
            "Repeat all",
            Action::SetRepeat(Repeat::One),
        ),
        (
            with(false, Repeat::One),
            "Repeat one",
            Action::SetRepeat(Repeat::Off),
        ),
    ];
    for (shown, label, want) in cases {
        assert_eq!(click_bar(&shown, true, label), [want], "{label}");
        assert_eq!(click_bar(&shown, false, label), [], "{label} before a play");
    }
    let on = bar(
        &egui::Context::default(),
        &with(true, Repeat::One),
        true,
        vec![],
    )
    .0;
    let toggled = |label: &str| {
        let node = on.iter().find(|(l, _)| l == label);
        node.and_then(|(_, n)| n.toggled())
    };
    let yes = Some(egui::accesskit::Toggled::True);
    assert_eq!((toggled("Shuffle"), toggled("Repeat one")), (yes, yes));
}

#[test]
fn a_long_title_and_artist_stay_one_line_each_and_show_in_full_on_hover() {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let long = "Synthetic Song One (feat. Fixture Artist, Fixture Guest and Yet Another Fixture Guest) - Extended Mix";
    let long_artist =
        "Fixture Artist, Fixture Guest, Another Fixture Guest, Yet Another Fixture Guest";
    let shown = |title: &str, artist: &str| {
        let mut shown = bramley_engine::Snapshot::default();
        (shown.title, shown.artist) = (Some(title.into()), Some(artist.into()));
        shown
    };
    let height = |nodes: &Nodes, label: &str| {
        let named = nodes.iter().filter(|(l, _)| l.starts_with(label));
        let tallest = named.filter_map(|(_, n)| n.bounds()).map(|r| r.y1 - r.y0);
        tallest.fold(f64::NAN, f64::max)
    };
    let (nodes, _) = bar(&ctx, &shown(long, long_artist), true, vec![]);
    let (one, _) = bar(&ctx, &shown("Short title", "Short artist"), true, vec![]);
    assert_eq!(
        (
            height(&nodes, "Synthetic Song One"),
            height(&nodes, "Fixture Artist,")
        ),
        (height(&one, "Short title"), height(&one, "Short artist"))
    );
    let hover = vec![Event::PointerMoved(centre(&nodes, long))];
    let shown_in_full = |_: u8| {
        let (nodes, _) = bar(&ctx, &shown(long, long_artist), true, hover.clone());
        nodes.iter().filter(|(l, _)| l == long).count()
    };
    let counts: Vec<_> = (0..5).map(shown_in_full).collect();
    assert!(counts.iter().any(|n| *n >= 2), "no tooltip: {counts:?}");
}

#[test]
fn the_footer_says_signed_in_above_sign_out_and_says_nothing_when_signed_out() {
    let labels = |signed_in| {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            super::sidebar::account(ui, signed_in);
        });
        out.textures_delta.clear();
        let tree = out.platform_output.accesskit_update.expect("a tree");
        let named = tree
            .nodes
            .into_iter()
            .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned));
        named.collect::<Vec<_>>()
    };
    let has = |labels: &[String], want: &str| labels.iter().any(|l| l == want);
    let (signed_in, signed_out) = (labels(true), labels(false));
    assert!(has(&signed_in, "Sign out") && has(&signed_in, "Signed in to Apple Music"));
    assert_eq!(signed_out, Vec::<String>::new());
}

const POSITION_MIN_PT: f32 = 64.0;
const CONTENT_MIN_PT: f32 = 320.0;

fn sized(width: f32, height: f32, events: Vec<Event>) -> RawInput {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    RawInput {
        screen_rect: Some(screen),
        events,
        ..RawInput::default()
    }
}

fn labelled(out: egui::FullOutput) -> Nodes {
    let tree = out.platform_output.accesskit_update.expect("a tree");
    let named = tree.nodes.into_iter();
    named
        .filter_map(|(_, n)| Some((n.label().or(n.value())?.to_owned(), n)))
        .collect()
}

fn rect_of(node: &egui::accesskit::Node) -> Option<egui::Rect> {
    let b = node.bounds()?;
    let min = egui::pos2(b.x0 as f32, b.y0 as f32);
    Some(egui::Rect::from_min_max(
        min,
        egui::pos2(b.x1 as f32, b.y1 as f32),
    ))
}

fn controls(nodes: &Nodes) -> Vec<(String, egui::Rect)> {
    use egui::accesskit::Role;
    let shown = nodes.iter().filter(|(l, n)| {
        let role = n.role();
        !l.is_empty()
            && matches!(
                role,
                Role::Button | Role::Slider | Role::TextInput | Role::Label
            )
    });
    let one_line = |r: &egui::Rect| r.height() <= 20.0;
    let rects = shown.filter_map(|(l, n)| Some((l.clone(), rect_of(n)?)));
    rects.filter(|(_, r)| one_line(r)).collect()
}

fn width_of(nodes: &Nodes, label: &str) -> f32 {
    let node = nodes.iter().find(|(l, _)| l == label);
    rect_of(&node.expect(label).1).expect(label).width()
}

fn assert_apart(nodes: &Nodes, (width, height): (f32, f32), at: &str) {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    let items = controls(nodes);
    for (label, rect) in &items {
        assert!(
            screen.expand(0.5).contains_rect(*rect),
            "{at}: {label} {rect:?} leaves the window"
        );
    }
    for (i, (a, ra)) in items.iter().enumerate() {
        for (b, rb) in &items[i + 1..] {
            let clash = ra.shrink(0.5).intersects(rb.shrink(0.5));
            assert!(!clash, "{at}: {a} {ra:?} overlaps {b} {rb:?}");
        }
    }
}

#[test]
fn the_player_bar_keeps_every_control_apart_and_on_screen_from_phone_to_desktop_widths() {
    let shown = bramley_engine::Snapshot::default();
    let sizes = [
        (360.0, 110.0),
        (400.0, 110.0),
        (500.0, 110.0),
        (639.0, 110.0),
    ];
    let wide = [640.0, 700.0, 800.0, 816.0, 900.0, 1100.0, 1280.0].map(|w| (w, 60.0));
    for (width, height) in sizes.into_iter().chain(wide) {
        let ctx = egui::Context::default();
        let nodes = bar_sized(&ctx, &shown, (true, 50), (width, height), vec![]).0;
        assert_apart(&nodes, (width, height), &format!("bar at {width}"));
        let position = width_of(&nodes, "Position");
        assert!(
            position >= POSITION_MIN_PT,
            "{width}: position is {position}"
        );
    }
}

#[test]
fn below_the_compact_width_the_bar_puts_title_and_queue_above_the_transport() {
    let (ctx, shown) = (
        egui::Context::default(),
        bramley_engine::Snapshot::default(),
    );
    let top = |width: f32, label: &str| {
        let nodes = bar_sized(&ctx, &shown, (true, 50), (width, 110.0), vec![]).0;
        let node = nodes.iter().find(|(l, _)| l == label);
        rect_of(&node.expect(label).1).expect(label).top()
    };
    assert!(top(400.0, "Queue") + 10.0 < top(400.0, "Play"));
    assert!((top(700.0, "Queue") - top(700.0, "Play")).abs() < 10.0);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn a_narrow_bar_folds_the_volume_into_an_icon_that_opens_a_slider_popup() {
    let (ctx, shown) = (
        egui::Context::default(),
        bramley_engine::Snapshot::default(),
    );
    let size = (700.0, 80.0);
    let nodes = bar_sized(&ctx, &shown, (true, 50), size, vec![]).0;
    assert!(nodes.iter().all(|(l, _)| l != "Volume"));
    let at = centre(&nodes, "Volume menu");
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    bar_sized(
        &ctx,
        &shown,
        (true, 50),
        size,
        vec![Event::PointerMoved(at), button(true)],
    );
    bar_sized(&ctx, &shown, (true, 50), size, vec![button(false)]);
    let open = bar_sized(&ctx, &shown, (true, 50), size, vec![]).0;
    assert_eq!(volume_of(&open), Some(50.0));
    let wide = bar_sized(
        &egui::Context::default(),
        &shown,
        (true, 50),
        (1100.0, 80.0),
        vec![],
    )
    .0;
    assert!(wide.iter().all(|(l, _)| l != "Volume menu"));
}

#[test]
#[cfg(target_os = "macos")]
fn no_bar_width_shows_any_volume_control_on_macos() {
    let shown = bramley_engine::Snapshot::default();
    for width in [400.0, 700.0, 1100.0] {
        let ctx = egui::Context::default();
        let nodes = bar_sized(&ctx, &shown, (true, 50), (width, 80.0), vec![]).0;
        assert!(nodes.iter().all(|(l, _)| !l.starts_with("Volume")));
    }
}

fn tap_at(ctx: &egui::Context, app: &mut crate::app::App, size: (f32, f32), at: egui::Pos2) {
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for events in [
        vec![Event::PointerMoved(at), button(true)],
        vec![button(false)],
    ] {
        frame(ctx, app, sized(size.0, size.1, events));
    }
}

fn shown(ctx: &egui::Context, app: &mut crate::app::App, size: (f32, f32)) -> Nodes {
    for _ in 0..2 {
        frame(ctx, app, sized(size.0, size.1, vec![]));
    }
    labelled(frame(ctx, app, sized(size.0, size.1, vec![])))
}

fn button_at(nodes: &Nodes, label: &str) -> Option<egui::Rect> {
    let found = nodes
        .iter()
        .find(|(l, n)| l == label && n.role() == egui::accesskit::Role::Button);
    found.and_then(|(_, n)| rect_of(n))
}

fn open_app(size: (f32, f32), route: Route) -> (egui::Context, crate::app::App) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = app(&ctx, true);
    app.go(route);
    shown(&ctx, &mut app, size);
    (ctx, app)
}

fn text_at(nodes: &Nodes, label: &str) -> Option<egui::Rect> {
    let found = nodes
        .iter()
        .find(|(l, n)| l == label && n.role() == egui::accesskit::Role::Label);
    found.and_then(|(_, n)| rect_of(n))
}

#[test]
fn an_open_queue_never_leaves_the_content_under_320_it_overlays_instead() {
    let widths = [
        360.0, 400.0, 639.0, 640.0, 700.0, 779.0, 780.0, 900.0, 1280.0,
    ];
    for width in widths {
        let size = (width, 600.0);
        let (ctx, mut app) = open_app(size, Route::Search);
        let nodes = shown(&ctx, &mut app, size);
        let queue = button_at(&nodes, "Queue").expect("the queue toggle");
        tap_at(&ctx, &mut app, size, queue.center());
        let nodes = shown(&ctx, &mut app, size);
        let field = nodes
            .iter()
            .find(|(_, n)| n.role() == egui::accesskit::Role::TextInput);
        let content = rect_of(&field.expect("the search field").1).expect("bounds");
        assert!(
            content.width() >= CONTENT_MIN_PT,
            "{width}: content is {}",
            content.width()
        );
        let empty = text_at(&nodes, "Nothing queued.").expect("the queue is open");
        let close = button_at(&nodes, "Close queue").expect("a close button");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0));
        assert!(
            screen.contains_rect(close),
            "{width}: close button off screen"
        );
        let sheet = width < super::COMPACT_PT;
        assert_eq!(
            sheet,
            empty.left() < 24.0,
            "{width}: the sheet covers the content"
        );
        let docked = empty.left() >= content.right();
        assert_eq!(
            docked,
            width >= 796.0,
            "{width}: docked only when 320 stays free"
        );
        tap_at(&ctx, &mut app, size, close.center());
        let nodes = shown(&ctx, &mut app, size);
        assert!(
            button_at(&nodes, "Close queue").is_none(),
            "{width}: the queue closes"
        );
    }
}

#[test]
fn the_compact_sidebar_is_a_drawer_that_opens_from_the_menu_and_closes_on_navigation() {
    let size = (400.0, 700.0);
    let (ctx, mut app) = open_app(size, Route::Home);
    let nodes = shown(&ctx, &mut app, size);
    assert!(
        button_at(&nodes, "Songs").is_none(),
        "the sidebar takes no width"
    );
    let menu = button_at(&nodes, "Menu").expect("a menu button");
    tap_at(&ctx, &mut app, size, menu.center());
    let nodes = shown(&ctx, &mut app, size);
    let songs = button_at(&nodes, "Songs").expect("the drawer lists the routes");
    assert!(
        songs.right() <= 300.0,
        "the drawer leaves the content peeking: {songs:?}"
    );
    tap_at(&ctx, &mut app, size, songs.center());
    assert_eq!(app.status_json()["route"], "Songs");
    let nodes = shown(&ctx, &mut app, size);
    assert!(
        button_at(&nodes, "Songs").is_none(),
        "choosing a route closes the drawer"
    );
    tap_at(&ctx, &mut app, size, menu.center());
    shown(&ctx, &mut app, size);
    tap_at(&ctx, &mut app, size, egui::pos2(380.0, 300.0));
    let nodes = shown(&ctx, &mut app, size);
    assert!(
        button_at(&nodes, "Songs").is_none(),
        "a tap outside closes the drawer"
    );
    let (ctx, mut app) = open_app((700.0, 500.0), Route::Home);
    let nodes = shown(&ctx, &mut app, (700.0, 500.0));
    assert!(button_at(&nodes, "Menu").is_none() && button_at(&nodes, "Songs").is_some());
}

#[test]
fn the_drawer_state_toggles_and_any_route_change_closes_it() {
    let mut shell = Shell::default();
    shell.apply(Nav::ToggleDrawer);
    assert!(shell.drawer_open);
    shell.apply(Nav::Go(Route::Songs));
    assert!(!shell.drawer_open);
    shell.apply(Nav::ToggleDrawer);
    shell.apply(Nav::Go(Route::Songs));
    assert!(
        !shell.drawer_open,
        "choosing the current route closes it too"
    );
}

#[test]
fn no_route_has_overlapping_controls_at_the_minimum_phone_and_small_desktop_sizes() {
    let [min_w, min_h] = super::MIN_WINDOW;
    let sizes = [
        (min_w, min_h),
        (400.0, 700.0),
        (640.0, 480.0),
        (700.0, 500.0),
        (1280.0, 800.0),
    ];
    let routes = [
        Route::Home,
        Route::Search,
        Route::Artists,
        Route::Albums,
        Route::Songs,
        Route::Playlists,
    ];
    for size in sizes {
        for route in routes.clone() {
            let (ctx, mut app) = open_app(size, route.clone());
            let nodes = shown(&ctx, &mut app, size);
            assert_apart(&nodes, size, &format!("{route:?} at {size:?}"));
        }
    }
}

fn focus_after((k, tabs): (Key, usize), hooked: bool) -> (Option<egui::Id>, Option<egui::Id>) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx, true);
    let focused = || ctx.memory(|m| m.focused());
    let mut start = None;
    for _ in 0..tabs {
        unhooked(&ctx, &mut app, press(Key::Tab, Modifiers::NONE));
        unhooked(&ctx, &mut app, RawInput::default());
        start = focused();
    }
    let step = if hooked { frame } else { unhooked };
    step(&ctx, &mut app, press(k, Modifiers::NONE));
    step(&ctx, &mut app, RawInput::default());
    (start, focused())
}

#[test]
fn tab_and_arrows_never_reach_egui_focus_outside_a_text_field() {
    let cases = [(Key::Tab, 3), (Key::Escape, 3), (Key::ArrowUp, 4)];
    let cases = cases.into_iter().chain([
        (Key::ArrowDown, 4),
        (Key::ArrowRight, 4),
        (Key::ArrowLeft, 10),
    ]);
    for case in cases {
        let (start, end) = focus_after(case, false);
        assert!(
            start.is_some() && start != end,
            "{case:?} must move focus unhooked"
        );
        let (start, end) = focus_after(case, true);
        assert_eq!(start, end, "{case:?} moved focus through the hook");
    }
}

fn typed_in_search(text: &str) -> (egui::Context, crate::app::App) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx, true);
    frame(&ctx, &mut app, press(Key::L, Modifiers::COMMAND));
    frame(&ctx, &mut app, RawInput::default());
    let typed = RawInput {
        events: vec![Event::Text(text.into())],
        ..RawInput::default()
    };
    frame(&ctx, &mut app, typed);
    assert!(ctx.text_edit_focused());
    (ctx, app)
}

fn caret(ctx: &egui::Context) -> Option<usize> {
    let state = egui::text_edit::TextEditState::load(ctx, search_id())?;
    Some(state.cursor.char_range()?.primary.index.into())
}

#[test]
fn every_bound_key_typed_into_search_fires_nothing() {
    let chords = BINDINGS.iter().flat_map(|b| b.keys);
    for &(k, modifiers) in chords {
        let (ctx, mut app) = typed_in_search("ab");
        frame(&ctx, &mut app, press(k, modifiers));
        let status = app.status_json();
        assert_eq!(status["route"], "Search", "{k:?}");
        assert_eq!(status["engineStarted"], false, "{k:?}");
        let expected = match k {
            Key::ArrowUp | Key::Home => 0,
            Key::ArrowLeft => 1,
            _ => 2,
        };
        assert_eq!(caret(&ctx), Some(expected), "{k:?}");
        assert_eq!(ctx.text_edit_focused(), k != Key::Escape, "{k:?}");
    }
}

#[test]
fn a_focused_slider_or_an_open_popup_keeps_arrows_and_escape_and_taking_is_idempotent() {
    let ctx = egui::Context::default();
    let slider = egui::Id::new("slider");
    super::keys::mark_slider(&ctx, slider);
    let taken = |ctx: &egui::Context| {
        let events = [Key::ArrowLeft, Key::Escape, Key::Tab].map(|k| key(k, true, Modifiers::NONE));
        let mut input = RawInput {
            events: events.to_vec(),
            ..RawInput::default()
        };
        super::keys::take(ctx, &mut input);
        let once = input.events.len();
        super::keys::take(ctx, &mut input);
        assert_eq!(input.events.len(), once);
        3 - once
    };
    assert_eq!(taken(&ctx), 3);
    ctx.memory_mut(|m| m.request_focus(slider));
    assert_eq!(taken(&ctx), 1);
    ctx.memory_mut(|m| m.surrender_focus(slider));
    egui::Popup::open_id(&ctx, egui::Id::new("menu"));
    assert_eq!(taken(&ctx), 1);
}

#[test]
fn down_arrows_focus_queue_rows_and_play_nothing() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut shown = bramley_engine::Snapshot::default();
    shown.queue = (0..40)
        .map(|n| bramley_engine::QueueItem {
            id: format!("i.{n}"),
            title: Some(format!("Row {n}")),
            duration: None,
        })
        .collect();
    shown.queue_length = 40;
    let (mut actions, mut navs) = (Vec::new(), Vec::new());
    let mut label = None;
    for n in 0..16 {
        let input = if n % 2 == 0 {
            hooked(&ctx, Key::ArrowDown)
        } else {
            RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            super::queue_panel::show(ui, &shown, (&mut actions, &mut navs));
        });
        out.textures_delta.clear();
        super::cursor::settle(&ctx);
        label = focus_label(&out.platform_output.accesskit_update.expect("a tree"));
    }
    assert_eq!(label.as_deref(), Some("Play Row 7"));
    assert!(actions.is_empty() && navs.is_empty());
}
