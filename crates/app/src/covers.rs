use std::collections::BTreeMap;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
pub use bramley_api::art_url;
use bramley_engine::{redact, user_agent};
use egui::{ColorImage, TextureHandle, TextureOptions};
use serde_json::Value;

pub const COVER_PT: f32 = 96.0;
pub const API_BASE: &str = "https://api.music.apple.com";
const STOREFRONT: &str = "/v1/me/storefront";
const CHART: &str = "/v1/catalog/us/charts?types=albums&limit=50";
const ART_HOST: &str = ".mzstatic.com";
const DECODERS: usize = 2;
const COUNT: usize = 50;
const MAX_BODY_BYTES: u64 = 4 << 20;

pub const DEVELOPER_REJECTED: &str = "Bramley's developer token was rejected";

#[derive(Debug)]
struct DeveloperTokenRejected;

impl std::fmt::Display for DeveloperTokenRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(DEVELOPER_REJECTED)
    }
}

impl std::error::Error for DeveloperTokenRejected {}

pub enum Loaded {
    Cover(usize, ColorImage),
    Failed(String, usize),
    SignInNeeded,
    DeveloperRejected,
}

#[derive(Default)]
pub struct Tally {
    in_flight: AtomicUsize,
    requested: AtomicUsize,
    cancel: AtomicBool,
}

#[derive(Clone)]
pub struct Source {
    pub api: String,
    pub art_prefix: Option<String>,
    pub user: crate::credentials::Credentials,
}

pub fn px(points: f32, pixels_per_point: f32) -> u32 {
    (points * pixels_per_point).round() as u32
}

pub fn allowed_art(url: &str, prefix: Option<&str>) -> bool {
    let Ok(uri) = url.parse::<ureq::http::Uri>() else {
        return false;
    };
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let origin = |u: &ureq::http::Uri| {
        (
            u.scheme_str().map(str::to_owned),
            u.host().map(str::to_owned),
            u.port_u16(),
        )
    };
    if let Some(Ok(base)) = prefix.map(str::parse::<ureq::http::Uri>)
        && origin(&base) == origin(&uri)
    {
        return true;
    }
    uri.scheme_str() == Some("https")
        && !authority.contains(':')
        && uri.host().is_some_and(|h| h.ends_with(ART_HOST))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(user_agent())
        .timeout_global(Some(Duration::from_secs(30)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    tally: &Tally,
) -> Result<(u16, Vec<u8>)> {
    if tally.cancel.load(Ordering::SeqCst) {
        bail!("cancelled");
    }
    tally.requested.fetch_add(1, Ordering::SeqCst);
    let request = headers
        .iter()
        .fold(agent.get(url), |r, (k, v)| r.header(*k, *v));
    let mut response = request.call()?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()?;
    Ok((status, body))
}

fn apple_error(status: u16, body: &[u8]) -> anyhow::Error {
    let body: Value = serde_json::from_slice(body).unwrap_or_default();
    let detail = &body["errors"][0];
    match detail["detail"].as_str().or(detail["title"].as_str()) {
        Some(why) => anyhow!("HTTP {status}: {why}"),
        None => anyhow!("HTTP {status}"),
    }
}

fn signed_in(
    agent: &ureq::Agent,
    source: &Source,
    bearer: &str,
    user: &str,
    tally: &Tally,
) -> Result<bool> {
    let headers = [("Authorization", bearer), ("Music-User-Token", user)];
    match get(
        agent,
        &format!("{}{STOREFRONT}", source.api),
        &headers,
        tally,
    )
    .context("sign-in check")?
    {
        (200, _) => Ok(true),
        (401, _) => Err(anyhow!(DeveloperTokenRejected)),
        (403, _) => Ok(false),
        (status, body) => Err(apple_error(status, &body).context("sign-in check")),
    }
}

fn chart(
    agent: &ureq::Agent,
    source: &Source,
    bearer: &str,
    px: u32,
    tally: &Tally,
) -> Result<(Vec<(usize, String)>, usize)> {
    let (status, body) = get(
        agent,
        &format!("{}{CHART}", source.api),
        &[("Authorization", bearer)],
        tally,
    )
    .context("chart")?;
    if status != 200 {
        return Err(apple_error(status, &body).context("chart"));
    }
    let body: Value = serde_json::from_slice(&body).context("chart")?;
    let albums = body["results"]["albums"][0]["data"]
        .as_array()
        .context("the chart has no albums")?;
    let usable = |album: &Value| {
        let url = art_url(album["attributes"]["artwork"]["url"].as_str()?, px)?;
        allowed_art(&url, source.art_prefix.as_deref()).then_some(url)
    };
    let urls: Vec<_> = albums
        .iter()
        .enumerate()
        .filter_map(|(i, a)| Some((i, usable(a)?)))
        .collect();
    Ok((urls.clone(), albums.len() - urls.len()))
}

fn cover(agent: &ureq::Agent, url: &str, px: u32, tally: &Tally) -> Result<ColorImage> {
    let (status, bytes) = get(agent, url, &[], tally)?;
    match status {
        200 => {}
        300..=399 => bail!("redirected (HTTP {status}); redirects are not followed"),
        _ => bail!("HTTP {status}"),
    }
    let side = px.saturating_mul(2);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(side);
    limits.max_image_height = Some(side);
    limits.max_alloc = Some(u64::from(side).pow(2) * 8);
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Jpeg);
    reader.limits(limits);
    let rgb = reader.decode()?.to_rgb8();
    let size = [rgb.width() as usize, rgb.height() as usize];
    Ok(ColorImage::from_rgb(size, rgb.as_raw()))
}

type Chart = (Vec<(usize, String)>, usize);

fn load_chart(
    agent: &ureq::Agent,
    source: &Source,
    px: u32,
    tally: &Tally,
) -> Result<Option<Chart>> {
    let tokens = source.user.developer();
    let developer = tokens.get().map_err(|why| anyhow!(why))?;
    let Some(user) = source.user.load().map_err(|why| anyhow!(why))? else {
        return Ok(None);
    };
    let attempt = |developer: &str| {
        let bearer = format!("Bearer {developer}");
        let checked = signed_in(agent, source, &bearer, &user, tally).and_then(|ok| {
            ok.then(|| chart(agent, source, &bearer, px, tally))
                .transpose()
        });
        checked.map_err(|e| match e.is::<DeveloperTokenRejected>() {
            true => e,
            false => anyhow!(
                "{}",
                redact(&format!("{e:#}"), &[developer.into(), user.clone()])
            ),
        })
    };
    match attempt(&developer) {
        Err(e) if e.is::<DeveloperTokenRejected>() && tokens.refetch(&developer) => {
            attempt(&tokens.get().map_err(|why| anyhow!(why))?)
        }
        loaded => loaded,
    }
}

pub fn fetch(
    source: Source,
    px: u32,
    tally: Arc<Tally>,
    wake: impl Fn() + Clone + Send + 'static,
) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("covers".into())
        .spawn(move || {
            let agent = agent();
            match load_chart(&agent, &source, px, &tally) {
                Ok(None) => drop(tx.send(Loaded::SignInNeeded)),
                Err(e) if e.is::<DeveloperTokenRejected>() => {
                    drop(tx.send(Loaded::DeveloperRejected))
                }
                Err(e) => drop(tx.send(Loaded::Failed(format!("{e:#}"), COUNT))),
                Ok(Some((urls, skipped))) => {
                    if skipped > 0 {
                        let why = format!("chart: {skipped} albums had no usable artwork");
                        let _ = tx.send(Loaded::Failed(why, skipped));
                    }
                    decode_on_workers(agent, (urls, px), &tx, tally, wake.clone());
                }
            }
            wake();
        })
        .expect("spawn covers thread");
    rx
}

fn decode_on_workers(
    agent: ureq::Agent,
    (urls, px): (Vec<(usize, String)>, u32),
    tx: &Sender<Loaded>,
    tally: Arc<Tally>,
    wake: impl Fn() + Clone + Send + 'static,
) {
    let jobs = Arc::new(Mutex::new(urls.into_iter()));
    for n in 0..DECODERS {
        let (agent, jobs, tx, tally, wake) = (
            agent.clone(),
            jobs.clone(),
            tx.clone(),
            tally.clone(),
            wake.clone(),
        );
        std::thread::Builder::new()
            .name(format!("cover-{n}"))
            .spawn(move || {
                while let Some((index, url)) = jobs.lock().unwrap().next() {
                    let decoded =
                        catch_unwind(AssertUnwindSafe(|| cover(&agent, &url, px, &tally)));
                    let loaded =
                        match decoded.unwrap_or_else(|_| Err(anyhow!("the decoder panicked"))) {
                            Ok(image) => {
                                tally
                                    .in_flight
                                    .fetch_add(image.pixels.len() * 4, Ordering::SeqCst);
                                Loaded::Cover(index, image)
                            }
                            Err(e) => Loaded::Failed(format!("cover {index}: {e:#}"), 1),
                        };
                    if tx.send(loaded).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn cover decoder");
    }
}

#[derive(Default)]
pub struct Grid {
    source: Option<Source>,
    px: u32,
    rx: Option<Receiver<Loaded>>,
    textures: BTreeMap<usize, TextureHandle>,
    tally: Arc<Tally>,
    failed: Option<String>,
    failures: usize,
    rejected: bool,
    developer_rejected: bool,
    #[cfg(feature = "gate-sabotage")]
    kept: Vec<ColorImage>,
}

impl Grid {
    pub fn new(source: Option<Source>) -> Self {
        Grid {
            source,
            ..Grid::default()
        }
    }

    pub fn reset(&mut self) {
        self.tally.cancel.store(true, Ordering::SeqCst);
        *self = Grid::new(self.source.take());
    }

    pub fn follow(&mut self, ctx: &egui::Context, signed_in: bool) {
        if !signed_in {
            if self.rx.is_some() {
                self.reset();
            }
            return;
        }
        let px = px(COVER_PT, ctx.pixels_per_point());
        let Some(source) = self.source.clone().filter(|_| self.px != px) else {
            return;
        };
        self.reset();
        self.px = px;
        let waker = ctx.clone();
        self.rx = Some(fetch(source, px, self.tally.clone(), move || {
            waker.request_repaint()
        }));
    }

    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut any = false;
        for loaded in rx.try_iter() {
            any = true;
            match loaded {
                Loaded::Cover(index, image) => {
                    let bytes = image.pixels.len() * 4;
                    #[cfg(feature = "gate-sabotage")]
                    self.kept.push(image.clone());
                    let texture =
                        ctx.load_texture(format!("cover-{index}"), image, TextureOptions::LINEAR);
                    self.textures.insert(index, texture);
                    self.tally.in_flight.fetch_sub(bytes, Ordering::SeqCst);
                }
                Loaded::Failed(why, covers) => {
                    self.failed.get_or_insert(why);
                    self.failures += covers;
                }
                Loaded::SignInNeeded => self.rejected = true,
                Loaded::DeveloperRejected => self.developer_rejected = true,
            }
        }
        any
    }

    pub fn rejected(&self) -> bool {
        self.rejected
    }

    pub fn developer_rejected(&self) -> bool {
        self.developer_rejected
    }

    pub fn uploaded(&self) -> usize {
        self.textures.len()
    }

    pub fn in_flight(&self) -> usize {
        self.tally.in_flight.load(Ordering::SeqCst)
    }

    pub fn kept(&self) -> usize {
        #[cfg(feature = "gate-sabotage")]
        return self.kept.iter().map(|image| image.pixels.len() * 4).sum();
        #[cfg(not(feature = "gate-sabotage"))]
        0
    }

    pub fn requested(&self) -> usize {
        self.tally.requested.load(Ordering::SeqCst)
    }

    pub fn failures(&self) -> usize {
        self.failures
    }

    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        if let Some(why) = &self.failed {
            ui.label(egui::RichText::new(format!("Some covers did not load: {why}")).weak());
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for texture in self.textures.values() {
                    let size = egui::vec2(COVER_PT, COVER_PT);
                    ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                }
            });
        });
    }
}
