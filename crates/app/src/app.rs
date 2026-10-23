use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(feature = "dev")]
use anyhow::{Context, Result};
use bramley_engine::PlayerError;
use fastframe_text::TextRendering;
use serde_json::{Value, json};

use crate::Args;
use crate::cli::{self, Inbox, Request};
use crate::covers::{self, Grid, Source};
use crate::credentials::Credentials;
use crate::playback::{Action, Banner, Playback, Spawn, play_or_pause};
use crate::theme::{self, Palette, Theme};
use crate::ui::cursor::Step;
use crate::ui::detail::Detail;
use crate::ui::library::Library;
use crate::ui::nav::{Nav, Shell};
use crate::ui::search::Search;
use crate::{signin, ui};

const VOLUME_FILE: &str = "volume";
const REPAINT_EVERY: Duration = Duration::from_secs(1);
const MEMORY_ONLY: [&str; 5] = ["title", "artist", "queue", "queuePosition", "queueLength"];
const QUEUE_FIRST: &str = "Play something first; the queue starts with it.";
const TOP_UP_FAILED: &str = "More songs from your library could not be added to the queue.";
const NOT_A_LIST: &str = "scroll works in a library list: open playlists, albums, songs, artists, a library playlist or an artist first";
const STILL_CLEARING: &str =
    "signed out here, but the stored sign-in is still being removed; run sign-out again to check";

type TextDetector = Box<dyn FnOnce() -> TextRendering + Send>;

#[cfg(not(test))]
fn text_detector() -> TextDetector {
    Box::new(fastframe_text::detect)
}

#[cfg(test)]
fn text_detector() -> TextDetector {
    Box::new(TextRendering::platform_default)
}

#[cfg(test)]
thread_local! {
    pub(crate) static FONT_SETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn font_tweak(rendering: &TextRendering) -> egui::FontTweak {
    let mut tweak = egui::FontTweak::default();
    rendering.tweak(&mut tweak);
    tweak
}

fn apply_text(ctx: &egui::Context, rendering: &TextRendering, applied: Option<&TextRendering>) {
    if applied.is_none_or(|before| font_tweak(before) != font_tweak(rendering)) {
        #[cfg(test)]
        FONT_SETS.with(|n| n.set(n.get() + 1));
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        rendering.apply_to(&mut fonts);
        ctx.set_fonts(fonts);
    }
    ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
}

fn detect_text_in_background(ctx: &egui::Context, detect: TextDetector) -> Receiver<TextRendering> {
    let (tx, rx) = channel();
    let waker = ctx.clone();
    std::thread::spawn(move || {
        if tx.send(detect()).is_ok() {
            waker.request_repaint();
        }
    });
    rx
}

fn load_volume(dir: &Path) -> f32 {
    let text = std::fs::read_to_string(dir.join(VOLUME_FILE)).unwrap_or_default();
    let percent = text.trim().parse::<u8>().ok().filter(|p| *p <= 100);
    percent.map_or(1.0, |p| f32::from(p) / 100.0)
}

type Job = Receiver<Result<bool, &'static str>>;
type Waiting = Vec<(Instant, Sender<String>)>;

fn worker(job: impl FnOnce() -> Result<bool, &'static str> + Send + 'static) -> Job {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx
}

fn poll(job: &Job) -> Option<Result<bool, &'static str>> {
    match job.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("the keyring worker stopped")),
    }
}

struct StatusWriter {
    tx: Option<Sender<String>>,
    thread: Option<JoinHandle<()>>,
}

impl StatusWriter {
    fn new(path: PathBuf, what: &'static str) -> Self {
        let (tx, rx) = channel::<String>();
        let thread = std::thread::spawn(move || {
            while let Ok(mut status) = rx.recv() {
                status = rx.try_iter().last().unwrap_or(status);
                if let Err(e) = bramley_api::store::write_private(&path, status.as_bytes()) {
                    eprintln!("bramley: {what} not written: {e}");
                }
            }
        });
        StatusWriter {
            tx: Some(tx),
            thread: Some(thread),
        }
    }
}

impl Drop for StatusWriter {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(feature = "dev")]
struct Shot {
    path: PathBuf,
    after: Duration,
    asked: bool,
}

pub struct App {
    pub(crate) api: Option<Sender<bramley_api::Job>>,
    pub(crate) api_cache: Option<Arc<bramley_api::store::Store>>,
    #[cfg(feature = "dev")]
    api_port: Option<u16>,
    search: Search,
    library: Library,
    detail: Detail,
    #[cfg(feature = "dev")]
    click_cover: Option<usize>,
    playback: Playback,
    theme: Theme,
    saved_volume: u8,
    volume_file: Option<StatusWriter>,
    user: Credentials,
    sign_in: Option<(signin::Server, bool)>,
    notice: Option<&'static str>,
    agent_notice: bool,
    agent_asking: bool,
    generation: Arc<AtomicU64>,
    checking: Option<Job>,
    clearing: Option<(Job, Waiting)>,
    covers: Grid,
    actions: Vec<Action>,
    shell: Shell,
    navs: Vec<Nav>,
    inbox: Inbox,
    guarded: bool,
    status: Option<StatusWriter>,
    #[cfg(feature = "dev")]
    shot: Option<Shot>,
    #[cfg(feature = "dev")]
    jump_to: Option<usize>,
    #[cfg(feature = "dev")]
    scroll_end: Option<Instant>,
    #[cfg(feature = "dev")]
    queue_fixture: bool,
    frames: u64,
    renderer: Option<String>,
    text: Option<Receiver<TextRendering>>,
    text_applied: TextRendering,
    #[cfg(feature = "dev")]
    born: Instant,
    status_at: Option<Instant>,
    #[cfg(feature = "gate-sabotage")]
    leak: crate::Leak,
    #[cfg(feature = "gate-sabotage")]
    ballast: crate::Ballast,
}

impl App {
    pub fn new(
        ctx: &egui::Context,
        args: Args,
        spawn: Spawn,
        (user, signed_out): (Credentials, bool),
        (inbox, guarded): (Inbox, bool),
        source: Option<Source>,
    ) -> Self {
        Self::new_detecting_text(
            ctx,
            args,
            spawn,
            (user, signed_out),
            (inbox, guarded),
            source,
            text_detector(),
        )
    }

    pub(crate) fn new_detecting_text(
        ctx: &egui::Context,
        args: Args,
        spawn: Spawn,
        (user, signed_out): (Credentials, bool),
        (inbox, guarded): (Inbox, bool),
        source: Option<Source>,
        detect: TextDetector,
    ) -> Self {
        #[cfg(feature = "dev")]
        let choice = args.theme;
        #[cfg(not(feature = "dev"))]
        let choice = theme::Choice::default();
        let file = theme::colors_file(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"));
        let mut theme = Theme::new(choice, file, Instant::now());
        theme.tick(ctx, Instant::now());
        apply_text(ctx, &TextRendering::platform_default(), None);
        let text = detect_text_in_background(ctx, detect);
        #[cfg(feature = "dev")]
        let actions = args.play.into_iter().map(Action::PlaySong).collect();
        #[cfg(not(feature = "dev"))]
        let actions = Vec::new();
        #[allow(unused_mut)]
        let mut shell = Shell::default();
        #[allow(unused_mut)]
        let mut search = Search::new(source.as_ref().and_then(|s| s.art_prefix.clone()));
        #[allow(unused_mut)]
        let mut library = Library::new(source.as_ref().and_then(|s| s.art_prefix.clone()));
        #[cfg(feature = "dev")]
        {
            library.click_last = args.click_last;
            library.playlists_collapsed = args.playlists_collapsed;
        }
        #[allow(unused_mut)]
        let mut detail = Detail::new(source.as_ref().and_then(|s| s.art_prefix.clone()));
        #[cfg(feature = "dev")]
        {
            detail.clicks = args.click_track;
            search.click_first_playlist = args.click_first_playlist;
            library.click_row = args.click_row;
            library.jump = args
                .jump_letter
                .map(|letter| (ui::library::List::Artists, letter));
            (shell.route, shell.queue_open) = (args.route.unwrap_or_default(), args.queue_open);
            shell.drawer_open = args.drawer_open;
            if let Some(term) = &args.search {
                shell.query.clone_from(term);
                search.typed_once_signed_in = Some(term.clone());
            }
            search.click_first_song = args.click_first_song;
        }
        let art_cache = source.as_ref().and_then(|s| s.art_cache.clone());
        search.art_cache(art_cache.clone());
        library.art_cache(art_cache.clone());
        detail.art_cache(art_cache);
        let mut playback = Playback::new(spawn, signed_out);
        playback.apply(Action::Volume(load_volume(user.dir())));
        App {
            api: None,
            api_cache: source.as_ref().and_then(|s| s.api_cache.clone()),
            #[cfg(feature = "dev")]
            api_port: loopback_port(args.api_base.as_deref()),
            search,
            library,
            detail,
            #[cfg(feature = "dev")]
            click_cover: args.click_cover,
            theme,
            saved_volume: percent(playback.volume()),
            playback,
            volume_file: None,
            user,
            sign_in: None,
            notice: None,
            agent_notice: false,
            agent_asking: false,
            generation: Arc::default(),
            checking: None,
            clearing: None,
            covers: Grid::new(source),
            actions,
            shell,
            navs: Vec::new(),
            inbox,
            guarded,
            status: args.status.map(|path| StatusWriter::new(path, "status")),
            #[cfg(feature = "dev")]
            shot: args.screenshot.map(|path| Shot {
                path,
                after: args.after,
                asked: false,
            }),
            #[cfg(feature = "dev")]
            jump_to: args.jump_to,
            #[cfg(feature = "dev")]
            scroll_end: args.scroll_end.map(|after| Instant::now() + after),
            #[cfg(feature = "dev")]
            queue_fixture: args.queue_fixture,
            frames: 0,
            renderer: None,
            text: Some(text),
            text_applied: TextRendering::platform_default(),
            #[cfg(feature = "dev")]
            born: Instant::now(),
            status_at: None,
            #[cfg(feature = "gate-sabotage")]
            leak: crate::Leak::from_env(),
            #[cfg(feature = "gate-sabotage")]
            ballast: crate::Ballast::from_env(),
        }
    }

    #[cfg(test)]
    pub(crate) fn scroll(&self) -> Option<ui::nav::Edge> {
        self.shell.scroll
    }

    #[cfg(test)]
    pub(crate) fn sign_in_accepted(&self) -> bool {
        let server = self.sign_in.as_ref().map(|(server, _)| server);
        server.is_some_and(|s| s.accepted.load(Ordering::SeqCst))
    }

    #[cfg(test)]
    pub(crate) fn apply(&mut self, action: Action) {
        self.playback.apply(action);
    }

    #[cfg(test)]
    pub(crate) fn go(&mut self, route: ui::nav::Route) {
        self.shell.apply(Nav::Go(route));
    }

    #[cfg(test)]
    pub(crate) fn nav(&mut self, nav: Nav) {
        self.shell.apply(nav);
    }

    #[cfg(test)]
    pub(crate) fn show_agent_notice(&mut self) {
        self.agent_notice = true;
    }

    pub fn status_json(&self) -> Value {
        let shown = self.playback.shown();
        let mut status = json!({
            "pid": std::process::id(),
            "engineStarted": self.playback.started(),
            "state": format!("{:?}", shown.state),
            "itemId": shown.item_id,
            "title": shown.title,
            "artist": shown.artist,
            "position": shown.position,
            "duration": shown.duration,
            "queue": shown.queue.iter().map(|item| json!({"id": item.id, "duration": item.duration})).collect::<Vec<_>>(),
            "queuePosition": shown.queue_position,
            "queueLength": shown.queue_length,
            "applied": shown.applied,
            "drainsAcked": shown.drains_acked,
            "drainTimeouts": shown.drain_timeouts,
            "sentMs": shown.sent_us / 1000,
            "skippedMs": shown.skipped_us / 1000,
            "decryptedMs": shown.decrypted_us / 1000,
            "readinessDips": shown.readiness_dips,
            "decryptFails": shown.decrypt_fails,
            "outputUnderflows": shown.output_underflows,
            "seq": self.playback.seq(),
            "engine": self.playback.engine(),
            "instanceGuarded": self.guarded,
            "banner": self.banner().map(|b| format!("{b:?}")),
            "signInUrl": self.waiting(),
            "notice": self.notice,
            "agentNotice": self.agent_notice,
            "tokenStorage": self.user.storage(),
            "framesPainted": self.frames,
            "glRenderer": self.renderer,
            "texturesUploaded": self.covers.uploaded(),
            "decodedInFlight": self.covers.in_flight(),
            "decodedBytesKept": self.covers.kept(),
            "coversRequested": self.covers.requested(),
            "coversFailed": self.covers.failures(),
            "coversError": self.covers.failed(),
            "route": self.shell.route.title(),
            "queueOpen": self.shell.queue_open,
            "search": self.search.status(),
            "library": self.library.status(),
            "detail": self.detail.status(),
        });
        status["cache"] = bramley_api::store::counts();
        status["signInReason"] = json!(self.playback.sign_in_reason());
        status["shuffleMode"] = json!(if shown.shuffle { "on" } else { "off" });
        status["repeatMode"] = json!(shown.repeat.as_str());
        status["volume"] = json!(self.volume_percent());
        status["loadFailReason"] = match &shown.error {
            Some(PlayerError::LoadFailed(why)) => json!(why),
            _ => Value::Null,
        };
        #[cfg(feature = "dev")]
        {
            status["libraryHttp"] = shown.http_statuses("/v1/me/library/songs");
        }
        #[cfg(feature = "gate-sabotage")]
        {
            status["ballastMiB"] = json!(self.ballast.mib());
        }
        if let (Some(status), Value::Object(token)) =
            (status.as_object_mut(), self.user.developer().status())
        {
            status.extend(token);
        }
        status
    }

    fn write_status(&mut self) {
        let Some(tx) = self.status.as_ref().and_then(|w| w.tx.as_ref()) else {
            return;
        };
        let mut status = self.status_json();
        if let Some(fields) = status.as_object_mut() {
            fields.retain(|key, _| !MEMORY_ONLY.contains(&key.as_str()));
        }
        let _ = tx.send(status.to_string());
        self.status_at = Some(Instant::now());
    }

    pub fn pump(&mut self, ctx: &egui::Context) -> bool {
        let changed = self.playback.receive()
            | self.covers.receive(ctx)
            | self.search.receive(ctx)
            | self.library.receive(ctx)
            | self.detail.receive(ctx)
            | self.answer_requests(ctx)
            | self.follow_sign_in()
            | self.follow_account();
        self.save_volume();
        changed
    }

    fn volume_percent(&self) -> u8 {
        percent(self.playback.volume())
    }

    fn shut_down(&mut self) {
        self.save_volume();
        self.playback.shutdown();
        self.status = None;
        self.volume_file = None;
    }

    fn save_volume(&mut self) {
        let percent = self.volume_percent();
        if percent == self.saved_volume {
            return;
        }
        self.saved_volume = percent;
        let dir = self.user.dir();
        let file = self.volume_file.get_or_insert_with(|| {
            let _ = std::fs::create_dir_all(dir);
            StatusWriter::new(dir.join(VOLUME_FILE), "volume")
        });
        if let Some(tx) = &file.tx {
            let _ = tx.send(format!("{percent}\n"));
        }
    }

    pub fn check_sign_in(&mut self) {
        self.playback.mark_signed_out("sign-in-check");
        let user = self.user.clone();
        self.checking = Some(worker(move || user.signed_in()));
    }

    fn follow_account(&mut self) -> bool {
        let checked = self.checking.as_ref().and_then(poll);
        match checked {
            Some(Ok(true)) => self.playback.signed_in(),
            Some(Ok(false)) => self.playback.relabel_signed_out("boot-no-token"),
            Some(Err(why)) => {
                self.playback.relabel_signed_out("keyring-unavailable");
                self.notice = Some(why);
            }
            None => {}
        }
        if checked.is_some() {
            self.checking = None;
        }
        let Some((job, waiting)) = self.clearing.as_mut() else {
            return checked.is_some();
        };
        let (cleared, now) = (poll(job), Instant::now());
        let due: Vec<_> = waiting
            .extract_if(.., |(at, _)| cleared.is_some() || *at <= now)
            .map(|(_, reply)| reply)
            .collect();
        if cleared.is_some() {
            self.clearing = None;
            self.wipe_api_cache();
        }
        if let Some(Err(why)) = cleared {
            self.notice = Some(why);
        }
        let mut answer = self.status_json();
        match cleared {
            None => answer["error"] = json!(STILL_CLEARING),
            Some(Err(why)) => answer["error"] = json!(why),
            Some(Ok(_)) => {}
        }
        for reply in due {
            let _ = reply.send(answer.to_string());
        }
        checked.is_some() || cleared.is_some()
    }

    fn answer_requests(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        while let Ok((request, deadline, reply)) = self.inbox.try_recv() {
            any = true;
            let late = Instant::now() > deadline;
            if matches!(request, Request::Agent | Request::Library(_)) {
                self.agent_notice |= !self.user.dir().join(crate::mcp::DISCLOSED).exists();
            }
            if request == Request::Agent {
                self.agent_asking = true;
                continue;
            }
            let from_agent = std::mem::take(&mut self.agent_asking);
            if let (Request::Search(term), false, false) = (&request, from_agent, late) {
                self.shell.apply(Nav::Go(ui::nav::Route::Search));
                self.shell.query.clone_from(term);
                self.search
                    .enter_by(term, Instant::now(), deadline + cli::REPLY_MARGIN);
            }
            if let Some(call) = cli::api_call(&request)
                && !late
            {
                let job = bramley_api::Job::new(call, deadline, move |found| {
                    let _ = reply.send(cli::api_reply(found));
                });
                self.send_api(job);
                continue;
            }
            let show = request == Request::Show;
            let outcome = match request {
                _ if late => Some(("error", cli::LATE)),
                Request::SignIn => self.start_sign_in().err().map(|why| ("error", why)),
                Request::SignOut => {
                    self.sign_out();
                    if let Some((_, waiting)) = self.clearing.as_mut() {
                        waiting.push((deadline, reply));
                    }
                    continue;
                }
                Request::Nav(Nav::Scroll(_)) if !self.shell.route.lists() => {
                    Some(("error", NOT_A_LIST))
                }
                Request::Nav(nav) => {
                    self.shell.apply(nav);
                    None
                }
                request => cli::command(&mut self.playback, request, deadline),
            };
            if show && outcome.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            let mut answer = self.status_json();
            if let Some((key, why)) = outcome {
                answer[key] = json!(why);
            }
            let _ = reply.send(answer.to_string());
        }
        any
    }

    fn dismiss_agent_notice(&mut self) {
        match crate::mcp::record_disclosed(self.user.dir()) {
            Ok(()) => self.agent_notice = false,
            Err(e) => eprintln!("bramley: the agent notice could not be recorded: {e}"),
        }
    }

    fn send_api(&mut self, job: bramley_api::Job) {
        let worker = match self.api.take() {
            Some(api) => Ok(api),
            None => {
                let (user, developer) = (self.user.clone(), self.user.developer().clone());
                let tokens = move || crate::api_tokens(&user);
                #[cfg(feature = "dev")]
                let client = match self.api_port {
                    Some(port) => bramley_api::Client::loopback(port, tokens),
                    None => bramley_api::Client::new(tokens),
                };
                #[cfg(not(feature = "dev"))]
                let client = bramley_api::Client::new(tokens);
                let client = match self.api_cache.clone() {
                    Some(store) => client.caching(store),
                    None => client,
                };
                bramley_api::worker(client.renewing(move |old| developer.refetch(old)))
            }
        };
        let unsent = match worker {
            Ok(api) => api
                .send(job)
                .map(|()| self.api = Some(api))
                .err()
                .map(|e| e.0),
            Err(_) => Some(job),
        };
        if let Some(job) = unsent {
            let stopped = "the API worker could not run".into();
            (job.done)(Err(bramley_api::Error::Network(stopped)));
        }
    }

    fn waiting(&self) -> Option<&str> {
        let (server, _) = self.sign_in.as_ref().filter(|(_, done)| !done)?;
        Some(&server.url)
    }

    fn start_sign_in(&mut self) -> Result<(), &'static str> {
        if self.waiting().is_some() {
            return Ok(());
        }
        if self.user.native() {
            let user = self.user.clone();
            (self.checking, self.notice) = (Some(worker(move || user.request())), None);
            return Ok(());
        }
        let developer = self.user.developer().peek();
        let developer = developer
            .ok_or("Bramley's developer token is still being fetched; try sign-in again shortly")?;
        let (user, generation) = (self.user.clone(), self.generation.clone());
        let at = generation.load(Ordering::SeqCst);
        let keep: signin::Keep = Box::new(move |token| {
            let current = || generation.load(Ordering::SeqCst) == at;
            let kept = user.save(token, current);
            kept.inspect_err(|why| eprintln!("bramley: the sign-in was not kept: {why}"))
        });
        let server = signin::start(&developer, signin::LIFETIME, keep)
            .map_err(|_| "the sign-in page could not start")?;
        println!("bramley: sign in to Apple Music at {}", server.url);
        #[cfg(not(test))]
        {
            let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
            let dir = dir.unwrap_or_else(|| self.user.dir().to_owned());
            if let Ok(page) = signin::redirect_file(&dir, &server.url) {
                signin::open_browser(&page);
            }
        }
        (self.sign_in, self.notice) = (Some((server, false)), None);
        Ok(())
    }

    fn follow_sign_in(&mut self) -> bool {
        let Some((server, done)) = self.sign_in.as_mut() else {
            return false;
        };
        match server.done.try_recv() {
            Err(TryRecvError::Empty) => return false,
            Ok(()) => {
                *done = true;
                self.playback.signed_in();
                self.covers.reset();
            }
            Err(TryRecvError::Disconnected) => {
                if !*done {
                    self.notice = Some("the sign-in page expired; sign in again");
                }
                self.sign_in = None;
            }
        }
        true
    }

    fn wipe_api_cache(&self) {
        if let Some(store) = &self.api_cache {
            store.wipe();
        }
    }

    fn sign_out(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        (self.sign_in, self.checking, self.notice, self.api) = (None, None, None, None);
        self.playback.sign_out();
        self.covers.reset();
        self.library.reset();
        self.wipe_api_cache();
        let user = self.user.clone();
        let waiting = self.clearing.take().map(|(_, waiting)| waiting);
        let job = worker(move || user.clear().map(|()| false));
        self.clearing = Some((job, waiting.unwrap_or_default()));
    }

    fn banner(&self) -> Option<Banner> {
        if let Some(url) = self.waiting() {
            return Some(Banner::SigningIn(url.to_owned()));
        }
        let current = || self.user.developer().peek();
        let rejected = self.covers.rejected_developer_token();
        let developer = rejected.is_some_and(|token| current().as_deref() == Some(token));
        let missing = || self.user.developer().missing().map(Banner::Error);
        let banner = match self.playback.banner() {
            None | Some(Banner::SignInNeeded) if developer => {
                Some(Banner::Error(covers::DEVELOPER_REJECTED.into()))
            }
            banner => banner,
        };
        let banner = banner.or_else(missing);
        banner.filter(|b| self.checking.is_none() || *b != Banner::SignInNeeded)
    }

    fn apply_detected_text(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.text else { return };
        match rx.try_recv() {
            Ok(rendering) if rendering != self.text_applied => {
                apply_text(ctx, &rendering, Some(&self.text_applied));
                self.text_applied = rendering;
            }
            Ok(_) => {}
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {}
        }
        self.text = None;
    }

    #[cfg(test)]
    pub(crate) fn text_settled(&self) -> bool {
        self.text.is_none()
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        self.tick_at(ctx, Instant::now());
    }

    pub(crate) fn tick_at(&mut self, ctx: &egui::Context, now: Instant) {
        let changed = self.pump(ctx);
        self.theme.tick(ctx, Instant::now());
        self.apply_detected_text(ctx);
        #[cfg(feature = "gate-sabotage")]
        self.leak.follow(self.playback.shown().item_id.as_deref());
        #[cfg(feature = "gate-sabotage")]
        self.ballast.follow();
        if self.covers.rejected() {
            self.playback.mark_signed_out("covers-403");
        }
        if self.library.denied {
            self.playback.mark_signed_out("library-denied");
        }
        self.covers.follow(ctx, covers_allowed(&self.playback));
        if !covers_allowed(&self.playback) {
            self.shell.scroll = None;
        }
        let waker = ctx.clone();
        let home = self.shell.route == ui::nav::Route::Home;
        self.covers.retry(ctx, now, home);
        let wake = move || waker.request_repaint();
        if let Some(job) = self.search.tick(now, covers_allowed(&self.playback), wake) {
            self.send_api(job);
        }
        let (recent_waker, signed_in) = (ctx.clone(), covers_allowed(&self.playback));
        let browsing =
            self.shell.route == ui::nav::Route::Search && self.shell.query.trim().is_empty();
        let wake = move || recent_waker.request_repaint();
        if let Some(job) = self.search.recent_job(now, signed_in && browsing, wake) {
            self.send_api(job);
        }
        if let Some(wait) = self.search.wait(now) {
            ctx.request_repaint_after(wait);
        }
        let waker = ctx.clone();
        let wake = move || waker.request_repaint();
        for job in (self.library).tick(now, covers_allowed(&self.playback), wake) {
            self.send_api(job);
        }
        let (route, waker) = (&self.shell.route, ctx.clone());
        let wake = move || waker.request_repaint();
        if let Some(job) = (self.detail).tick(now, covers_allowed(&self.playback), route, wake) {
            self.send_api(job);
        }
        #[cfg(feature = "dev")]
        if self.scroll_end.is_some_and(|at| now >= at) {
            self.shell.apply(Nav::Scroll(ui::nav::Edge::End));
            self.scroll_end = None;
        }
        #[cfg(feature = "dev")]
        if let Some(id) = self.click_cover.and_then(|n| self.covers.album(n)) {
            self.navs
                .push(Nav::Go(ui::nav::Route::Album(id.to_owned())));
            self.click_cover = None;
        }
        let queue = (
            self.playback.generation(),
            self.playback.background_settled(),
        );
        if let Some(more) = self.library.top_up(self.playback.snapshot(), queue, now) {
            self.playback.apply(more);
        }
        if std::mem::take(&mut self.library.top_up_failed) {
            self.notice = Some(TOP_UP_FAILED);
        }
        if changed
            || self
                .status_at
                .is_none_or(|at| at.elapsed() >= REPAINT_EVERY)
        {
            self.write_status();
        }
        #[cfg(feature = "dev")]
        self.drive_shot(ctx);
        #[cfg(feature = "dev")]
        self.drive_jump();
        ctx.request_repaint_after(REPAINT_EVERY);
    }

    fn overlay_nav(&self, docked: bool, compact: bool) -> Option<Nav> {
        (self.shell.queue_open && !docked)
            .then_some(Nav::ToggleQueue)
            .or((compact && self.shell.drawer_open).then_some(Nav::ToggleDrawer))
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        self.frames += 1;
        let (shown, started) = (self.playback.shown(), self.playback.started());
        let volume = self.volume_percent();
        let toggle = started.then(|| play_or_pause(shown.state));
        ui::shortcuts(ui.ctx(), toggle, &mut self.actions, &mut self.navs);
        self.actions
            .extend(ui::keys::actions(ui.ctx(), &shown, volume));
        let compact = ui.available_width() < ui::COMPACT_PT;
        for step in ui::cursor::drain(ui.ctx(), &[Step::Next, Step::Prev]) {
            let regions = (!compact, self.shell.queue_open);
            ui::cursor::cycle(ui.ctx(), step == Step::Prev, regions);
        }
        ui::cursor::walk(ui.ctx());
        let palette = Palette::of(ui.ctx());
        egui::Panel::bottom("now-playing").show(ui, |ui| {
            ui.add_space(8.0);
            let emit = (&mut self.actions, &mut self.navs);
            let layout = (self.shell.queue_open, compact);
            ui::player_bar::show(ui, &shown, started, volume, layout, emit);
            ui.add_space(8.0);
        });
        let (mut sign_in, mut sign_out, mut dismissed) = (false, false, false);
        let signed_in = covers_allowed(&self.playback);
        if !compact {
            let frame = egui::Frame::side_top_panel(ui.style()).fill(palette.sidebar);
            egui::Panel::left("library")
                .default_size(200.0)
                .frame(frame)
                .show(ui, |ui| {
                    let (route, library) = (&self.shell.route, &mut self.library);
                    sign_out = ui::sidebar::show(ui, route, library, &mut self.navs, signed_in);
                });
        }
        let free = ui.available_rect_before_wrap();
        let docked = self.shell.queue_open
            && !compact
            && free.width() >= ui::CONTENT_PANEL_MIN_PT + ui::QUEUE_PT;
        if docked {
            egui::Panel::right("queue")
                .default_size(ui::QUEUE_PT)
                .max_size(free.width() - ui::CONTENT_PANEL_MIN_PT)
                .show(ui, |ui| {
                    let emit = (&mut self.actions, &mut self.navs);
                    ui::queue_panel::show(ui, &shown, emit)
                });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(banner) = self.banner() {
                let needed = Some(Banner::SignInNeeded);
                let offer = Some(banner.clone()) == needed || self.playback.banner() == needed;
                sign_in = ui::banner(ui, &banner, offer);
            }
            if let Some(notice) = self.notice {
                ui::notice(ui, notice);
            }
            if let Some(warning) = self.user.developer().warning() {
                ui::notice(ui, &warning);
            }
            dismissed = self.agent_notice && crate::mcp::notice(ui).clicked();
            let view = (
                &mut self.shell,
                &mut self.search,
                &mut self.library,
                &mut self.detail,
            );
            let emit = (&mut self.actions, &mut self.navs);
            ui::content::show(ui, view, &self.covers, emit, compact);
        });
        if self.shell.queue_open && !docked {
            let mut sheet = free;
            if !compact {
                sheet.min.x = free.max.x - ui::QUEUE_PT.min(free.width());
            }
            ui::overlay(ui, "queue-overlay", (sheet, palette.panel), |ui| {
                let emit = (&mut self.actions, &mut self.navs);
                ui::queue_panel::show(ui, &shown, emit)
            });
        }
        if compact && self.shell.drawer_open {
            let mut drawer = free;
            drawer.max.x = free.min.x + ui::DRAWER_PT.min(free.width() * 0.8);
            let outside = ui::overlay(ui, "drawer", (drawer, palette.sidebar), |ui| {
                let (route, library) = (&self.shell.route, &mut self.library);
                sign_out = ui::sidebar::show(ui, route, library, &mut self.navs, signed_in);
            });
            if outside.clicked_elsewhere() {
                self.navs.push(Nav::ToggleDrawer);
            }
        }
        if self.shell.help_open {
            ui::overlay(ui, "help", (free, palette.panel), ui::keys::help);
        }
        self.library.art.flush(ui.ctx());
        for _ in ui::cursor::drain(ui.ctx(), &[Step::Help]) {
            self.navs.push(Nav::ToggleHelp);
        }
        for step in ui::cursor::drain(ui.ctx(), &[Step::Search, Step::Bottom]) {
            let end = Nav::Scroll(ui::nav::Edge::End);
            let nav = (step == Step::Search).then_some(Nav::FocusSearch);
            self.navs
                .extend(nav.or(self.shell.route.lists().then_some(end)));
        }
        for step in ui::cursor::drain(ui.ctx(), &[Step::Escape, Step::Back]) {
            let overlay = (step == Step::Escape)
                .then(|| self.overlay_nav(docked, compact))
                .flatten();
            self.navs.push(overlay.unwrap_or(Nav::Back));
        }
        ui::cursor::settle(ui.ctx());
        if sign_out {
            self.sign_out();
        }
        if dismissed {
            self.dismiss_agent_notice();
        }
        if let Some(Err(why)) = sign_in.then(|| self.start_sign_in()) {
            self.notice = Some(why);
            eprintln!("bramley: {why}");
        }
        if !self.navs.is_empty() {
            ui.ctx().request_repaint();
        }
        for nav in self.navs.drain(..) {
            self.shell.apply(nav);
        }
        ui::keys::modal(ui.ctx(), self.shell.help_open);
        if started && self.notice == Some(QUEUE_FIRST) {
            self.notice = None;
        }
        for action in self.actions.drain(..) {
            let queues = matches!(action, Action::PlayNext(_) | Action::QueueAdd(_));
            if !self.playback.apply(action) && queues {
                self.notice = Some(QUEUE_FIRST);
            }
        }
        self.library.started(self.playback.generation());
    }

    #[cfg(feature = "dev")]
    fn drive_jump(&mut self) {
        if self.queue_fixture && !self.playback.started() {
            self.playback.fixture(queue_fixture());
        }
        let Some(target) = self.jump_to else { return };
        let shown = self.playback.shown();
        let settled = shown.applied >= self.playback.seq() && shown.queue_length > target;
        let Some(at) = shown.queue_position.filter(|_| settled) else {
            return;
        };
        if target <= at {
            self.jump_to = None;
            return;
        }
        let row = (target - at).min(shown.queue.len().saturating_sub(1));
        self.actions.extend(ui::queue_panel::jump(&shown, row));
    }

    #[cfg(feature = "dev")]
    fn drive_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else {
            return;
        };
        ctx.request_repaint();
        if !shot.asked && self.frames > 0 && self.born.elapsed() >= shot.after {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.asked = true;
        }
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        if let Err(e) = write_png(&shot.path, &image) {
            eprintln!("bramley: screenshot not written: {e:#}");
        }
        self.shot = None;
        self.write_status();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.renderer.is_none() {
            self.renderer = renderer_name(frame);
        }
        crate::warm::remember_scale(ctx.pixels_per_point());
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        ui::keys::take(ctx, raw_input);
    }

    #[cfg(not(target_os = "macos"))]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shut_down();
    }

    #[cfg(target_os = "macos")]
    fn on_exit(&mut self) {
        self.shut_down();
    }
}

#[cfg(not(target_os = "macos"))]
fn renderer_name(frame: &eframe::Frame) -> Option<String> {
    use eframe::glow::HasContext;
    frame
        .gl()
        .map(|gl| unsafe { gl.get_parameter_string(eframe::glow::RENDERER) })
}

#[cfg(target_os = "macos")]
fn renderer_name(frame: &eframe::Frame) -> Option<String> {
    let info = frame.wgpu_render_state()?.adapter.get_info();
    Some(format!("{:?} {}", info.backend, info.name))
}

#[cfg(feature = "dev")]
fn loopback_port(base: Option<&str>) -> Option<u16> {
    let port = base?.strip_prefix("http://127.0.0.1:")?;
    port.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| port.parse().ok())?
}

fn percent(volume: f32) -> u8 {
    (volume * 100.0).round() as u8
}

pub fn covers_allowed(playback: &Playback) -> bool {
    playback.banner() != Some(Banner::SignInNeeded)
}

#[cfg(feature = "dev")]
fn queue_fixture() -> bramley_engine::Snapshot {
    let mut snap = bramley_engine::Snapshot::default();
    snap.queue = (1..=14)
        .map(|n| bramley_engine::QueueItem {
            id: format!("i.{n}"),
            title: Some(format!("Synthetic Song {n}")),
            duration: Some(150.0 + 17.0 * n as f64),
        })
        .collect();
    (snap.title, snap.artist) = (snap.queue[0].title.clone(), Some("Fixture Artist".into()));
    (snap.item_id, snap.duration) = (Some("i.1".into()), snap.queue[0].duration);
    (snap.queue_position, snap.queue_length, snap.position) = (Some(0), 120, Some(42.0));
    snap
}

#[cfg(feature = "dev")]
fn write_png(path: &Path, image: &egui::ColorImage) -> Result<()> {
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let [w, h] = image.size.map(|n| n as u32);
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
