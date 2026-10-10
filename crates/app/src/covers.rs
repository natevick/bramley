use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bramley_engine::{redact, user_agent};
use egui::{ColorImage, TextureHandle, TextureOptions};
use serde_json::Value;

pub const COVER_PT: f32 = 96.0;
pub const API_BASE: &str = "https://api.music.apple.com";
const CHART: &str = "/v1/catalog/us/charts?types=albums&limit=50";
const ART_HOST: &str = ".mzstatic.com";
const DECODERS: usize = 2;
const COUNT: usize = 50;
const MAX_ART_BYTES: u64 = 4 << 20;

pub enum Loaded {
    Cover(usize, ColorImage),
    Failed(String, usize),
}

#[derive(Default)]
pub struct Tally {
    held: AtomicUsize,
    requested: AtomicUsize,
}

#[derive(Clone)]
pub struct Source {
    pub api: String,
    pub art_prefix: Option<String>,
    pub config: PathBuf,
}

pub fn px(points: f32, pixels_per_point: f32) -> u32 {
    (points * pixels_per_point).round() as u32
}

pub fn art_url(template: &str, px: u32) -> Option<String> {
    let px = px.to_string();
    let url = template.replace("{w}", &px).replace("{h}", &px);
    (!url.contains(['{', '}'])).then_some(url)
}

pub fn allowed_art(url: &str, prefix: Option<&str>) -> bool {
    if prefix.is_some_and(|p| url.starts_with(p)) {
        return true;
    }
    let Ok(uri) = url.parse::<ureq::http::Uri>() else {
        return false;
    };
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or("");
    uri.scheme_str() == Some("https")
        && !authority.contains(['@', ':'])
        && uri.host().is_some_and(|h| h.ends_with(ART_HOST))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(user_agent())
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn chart(
    agent: &ureq::Agent,
    source: &Source,
    token: &str,
    px: u32,
    tally: &Tally,
) -> Result<Vec<String>> {
    tally.requested.fetch_add(1, Ordering::SeqCst);
    let body = agent
        .get(format!("{}{CHART}", source.api))
        .header("Authorization", format!("Bearer {token}"))
        .call()?
        .body_mut()
        .read_to_vec()?;
    let body: Value = serde_json::from_slice(&body)?;
    let albums = body["results"]["albums"][0]["data"]
        .as_array()
        .context("the chart has no albums")?;
    albums
        .iter()
        .map(|album| {
            let template = album["attributes"]["artwork"]["url"]
                .as_str()
                .context("an album has no artwork")?;
            match art_url(template, px) {
                Some(url) if allowed_art(&url, source.art_prefix.as_deref()) => Ok(url),
                _ => bail!("refused artwork URL {template:?}"),
            }
        })
        .collect()
}

fn cover(agent: &ureq::Agent, url: &str, px: u32, tally: &Tally) -> Result<ColorImage> {
    tally.requested.fetch_add(1, Ordering::SeqCst);
    let bytes = agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(MAX_ART_BYTES)
        .read_to_vec()?;
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
            let token = crate::read_token(&source.config, "developer-token");
            let urls = token.and_then(|token| {
                chart(&agent, &source, &token, px, &tally)
                    .map_err(|e| anyhow::anyhow!("{}", redact(&format!("chart: {e:#}"), &[token])))
            });
            match urls {
                Ok(urls) => decode_on_workers(agent, (urls, px), &tx, tally, wake),
                Err(e) => {
                    let _ = tx.send(Loaded::Failed(format!("{e:#}"), COUNT));
                    wake();
                }
            }
        })
        .expect("spawn covers thread");
    rx
}

fn decode_on_workers(
    agent: ureq::Agent,
    (urls, px): (Vec<String>, u32),
    tx: &Sender<Loaded>,
    tally: Arc<Tally>,
    wake: impl Fn() + Clone + Send + 'static,
) {
    let jobs = Arc::new(Mutex::new(urls.into_iter().enumerate()));
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
                loop {
                    let Some((index, url)) = jobs.lock().unwrap().next() else {
                        break;
                    };
                    let loaded = match cover(&agent, &url, px, &tally) {
                        Ok(image) => {
                            tally
                                .held
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
    rx: Option<Receiver<Loaded>>,
    textures: BTreeMap<usize, TextureHandle>,
    tally: Arc<Tally>,
    failed: Option<String>,
    failures: usize,
    #[cfg(feature = "gate-sabotage")]
    kept: Vec<ColorImage>,
}

impl Grid {
    pub fn follow(&mut self, ctx: &egui::Context, signed_in: bool, source: &mut Option<Source>) {
        if !signed_in {
            if self.rx.is_some() {
                *self = Grid::default();
            }
            return;
        }
        let Some(source) = source.take() else { return };
        let px = px(COVER_PT, ctx.pixels_per_point());
        let waker = ctx.clone();
        let tally = self.tally.clone();
        self.rx = Some(fetch(source, px, tally, move || waker.request_repaint()));
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
                    let name = format!("cover-{index}");
                    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.textures.insert(index, texture);
                    if !cfg!(feature = "gate-sabotage") {
                        self.tally.held.fetch_sub(bytes, Ordering::SeqCst);
                    }
                }
                Loaded::Failed(why, covers) => {
                    self.failed.get_or_insert(why);
                    self.failures += covers;
                }
            }
        }
        any
    }

    pub fn uploaded(&self) -> usize {
        self.textures.len()
    }

    pub fn held(&self) -> usize {
        self.tally.held.load(Ordering::SeqCst)
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
