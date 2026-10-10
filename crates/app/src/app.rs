#[cfg(feature = "dev")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(feature = "dev")]
use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::Args;
use crate::cli::{self, Inbox, Request};
use crate::covers::{self, Grid, Source};
use crate::credentials::Credentials;
use crate::playback::{Action, Banner, Playback, Spawn};
use crate::{signin, ui};

const REPAINT_EVERY: Duration = Duration::from_secs(1);

struct StatusWriter {
    tx: Option<Sender<String>>,
    thread: Option<JoinHandle<()>>,
}

impl StatusWriter {
    fn new(path: PathBuf) -> Self {
        let (tx, rx) = channel::<String>();
        let thread = std::thread::spawn(move || {
            while let Ok(mut status) = rx.recv() {
                status = rx.try_iter().last().unwrap_or(status);
                let partial = path.with_extension("partial");
                let written = std::fs::write(&partial, status);
                if let Err(e) = written.and_then(|()| std::fs::rename(&partial, &path)) {
                    eprintln!("bramley: status not written to {}: {e}", path.display());
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
    playback: Playback,
    user: Credentials,
    sign_in: Option<(signin::Server, bool)>,
    notice: Option<&'static str>,
    covers: Grid,
    actions: Vec<Action>,
    inbox: Inbox,
    guarded: bool,
    status: Option<StatusWriter>,
    #[cfg(feature = "dev")]
    shot: Option<Shot>,
    frames: u64,
    renderer: Option<String>,
    #[cfg(feature = "dev")]
    born: Instant,
    status_at: Option<Instant>,
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
        let rendering = fastframe_text::detect();
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        rendering.apply_to(&mut fonts);
        ctx.set_fonts(fonts);
        ctx.set_visuals(egui::Visuals::dark());
        ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
        #[cfg(feature = "dev")]
        let actions = args.play.into_iter().map(Action::PlaySong).collect();
        #[cfg(not(feature = "dev"))]
        let actions = Vec::new();
        App {
            api: None,
            playback: Playback::new(spawn, signed_out),
            user,
            sign_in: None,
            notice: None,
            covers: Grid::new(source),
            actions,
            inbox,
            guarded,
            status: args.status.map(StatusWriter::new),
            #[cfg(feature = "dev")]
            shot: args.screenshot.map(|path| Shot {
                path,
                after: args.after,
                asked: false,
            }),
            frames: 0,
            renderer: None,
            #[cfg(feature = "dev")]
            born: Instant::now(),
            status_at: None,
        }
    }

    pub fn status_json(&self) -> Value {
        let shown = self.playback.shown();
        json!({
            "pid": std::process::id(),
            "engineStarted": self.playback.started(),
            "state": format!("{:?}", shown.state),
            "itemId": shown.item_id,
            "title": shown.title,
            "artist": shown.artist,
            "position": shown.position,
            "duration": shown.duration,
            "applied": shown.applied,
            "seq": self.playback.seq(),
            "engine": self.playback.engine(),
            "instanceGuarded": self.guarded,
            "banner": self.banner().map(|b| format!("{b:?}")),
            "signInUrl": self.waiting(),
            "notice": self.notice,
            "framesPainted": self.frames,
            "glRenderer": self.renderer,
            "texturesUploaded": self.covers.uploaded(),
            "decodedInFlight": self.covers.in_flight(),
            "decodedBytesKept": self.covers.kept(),
            "coversRequested": self.covers.requested(),
            "coversFailed": self.covers.failures(),
            "coversError": self.covers.failed(),
        })
    }

    fn write_status(&mut self) {
        let Some(tx) = self.status.as_ref().and_then(|w| w.tx.as_ref()) else {
            return;
        };
        let _ = tx.send(self.status_json().to_string());
        self.status_at = Some(Instant::now());
    }

    pub fn pump(&mut self, ctx: &egui::Context) -> bool {
        self.playback.receive()
            | self.covers.receive(ctx)
            | self.answer_requests(ctx)
            | self.follow_sign_in()
    }

    fn answer_requests(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        while let Ok((request, deadline, reply)) = self.inbox.try_recv() {
            any = true;
            if let Request::Search(term) = &request
                && Instant::now() <= deadline
            {
                let call = bramley_api::Call::Search(term.clone());
                let job = bramley_api::Job::new(call, deadline, move |found| {
                    let _ = reply.send(cli::search_reply(found));
                });
                let worker = match self.api.take() {
                    Some(api) => Ok(api),
                    None => {
                        let user = self.user.clone();
                        bramley_api::worker(bramley_api::Client::new(move || {
                            crate::api_tokens(&user)
                        }))
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
                continue;
            }
            let show = request == Request::Show;
            let outcome = match request {
                Request::SignIn => self.start_sign_in().err().map(|why| ("error", why)),
                Request::SignOut => self.sign_out().err().map(|why| ("error", why)),
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

    fn waiting(&self) -> Option<&str> {
        let (server, _) = self.sign_in.as_ref().filter(|(_, done)| !done)?;
        Some(&server.url)
    }

    fn start_sign_in(&mut self) -> Result<(), &'static str> {
        if self.waiting().is_some() {
            return Ok(());
        }
        let developer = crate::read_token(self.user.dir(), "developer-token")
            .map_err(|_| "sign-in needs Bramley's developer token, which is missing")?;
        let user = self.user.clone();
        let keep = move |token: &str| {
            let kept = user.save(token);
            if !kept {
                eprintln!("bramley: the sign-in could not be stored");
            }
            kept
        };
        let server = signin::start(&developer, signin::LIFETIME, keep)
            .map_err(|_| "the sign-in page could not start")?;
        println!("bramley: sign in to Apple Music at {}", server.url);
        #[cfg(not(test))]
        signin::open_browser(&server.url);
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

    fn sign_out(&mut self) -> Result<(), &'static str> {
        self.sign_in = None;
        self.playback.shutdown();
        self.playback.mark_signed_out();
        self.covers.reset();
        self.notice =
            (!self.user.clear()).then_some("sign-out could not remove the stored sign-in");
        self.notice.map_or(Ok(()), Err)
    }

    fn banner(&self) -> Option<Banner> {
        if let Some(url) = self.waiting() {
            return Some(Banner::SigningIn(url.to_owned()));
        }
        let developer = self.covers.developer_rejected();
        let rejected = || Banner::Error(covers::DEVELOPER_REJECTED.into());
        self.playback.banner().or_else(|| developer.then(rejected))
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let changed = self.pump(ctx);
        if self.covers.rejected() {
            self.playback.mark_signed_out();
        }
        self.covers.follow(ctx, covers_allowed(&self.playback));
        if changed
            || self
                .status_at
                .is_none_or(|at| at.elapsed() >= REPAINT_EVERY)
        {
            self.write_status();
        }
        #[cfg(feature = "dev")]
        self.drive_shot(ctx);
        ctx.request_repaint_after(REPAINT_EVERY);
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        self.frames += 1;
        let shown = self.playback.shown();
        egui::Panel::bottom("now-playing").show(ui, |ui| {
            ui.add_space(8.0);
            ui::now_playing(ui, &shown, self.playback.started(), &mut self.actions);
            ui.add_space(8.0);
        });
        let (mut sign_in, mut sign_out) = (false, false);
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(banner) = self.banner() {
                sign_in = ui::banner(ui, &banner);
            }
            if let Some(notice) = self.notice {
                ui::notice(ui, notice);
            }
            ui.horizontal(|ui| {
                ui.heading("Bramley");
                sign_out = covers_allowed(&self.playback) && ui.button("Sign out").clicked();
            });
            self.covers.show(ui);
        });
        let outcome = match (sign_in, sign_out) {
            (true, _) => self.start_sign_in(),
            (_, true) => self.sign_out(),
            _ => Ok(()),
        };
        if let Err(why) = outcome {
            self.notice = Some(why);
            eprintln!("bramley: {why}");
        }
        for action in self.actions.drain(..) {
            self.playback.apply(action);
        }
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
            use eframe::glow::HasContext;
            self.renderer = frame
                .gl()
                .map(|gl| unsafe { gl.get_parameter_string(eframe::glow::RENDERER) });
        }
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.playback.shutdown();
        self.status = None;
    }
}

pub fn covers_allowed(playback: &Playback) -> bool {
    playback.banner() != Some(Banner::SignInNeeded)
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
