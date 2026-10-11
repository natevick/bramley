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
        shell.apply(Nav::Go([Route::Albums, Route::Songs][i % 2]));
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
    assert_eq!((shell.route, shell.focus_search), (Route::Search, true));
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

fn app(ctx: &egui::Context) -> crate::app::App {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/no-config");
    let spawn = crate::tests::spawning(|| unreachable!("no player may start in these tests"));
    let user = (crate::tests::mock_user(&dir), true);
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
    let mut app = app(&ctx);
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
    let mut app = app(&ctx);
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
