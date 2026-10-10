use std::collections::BTreeMap;
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
const MAX_ART_BYTES: u64 = 4 << 20;

pub enum Loaded {
    Cover(usize, ColorImage),
    Failed(String),
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

fn chart(agent: &ureq::Agent, source: &Source, token: &str, px: u32) -> Result<Vec<String>> {
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

fn cover(agent: &ureq::Agent, url: &str) -> Result<ColorImage> {
    let bytes = agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(MAX_ART_BYTES)
        .read_to_vec()?;
    let rgb = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    let size = [rgb.width() as usize, rgb.height() as usize];
    Ok(ColorImage::from_rgb(size, rgb.as_raw()))
}

pub fn fetch(
    source: Source,
    px: u32,
    held: Arc<AtomicUsize>,
    wake: impl Fn() + Clone + Send + 'static,
) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("covers".into())
        .spawn(move || {
            let agent = agent();
            let token = crate::read_token(&source.config, "developer-token");
            let urls = token.and_then(|token| {
                chart(&agent, &source, &token, px)
                    .map_err(|e| anyhow::anyhow!("{}", redact(&format!("chart: {e:#}"), &[token])))
            });
            match urls {
                Ok(urls) => decode_on_workers(agent, urls, &tx, held, wake),
                Err(e) => {
                    let _ = tx.send(Loaded::Failed(format!("{e:#}")));
                    wake();
                }
            }
        })
        .expect("spawn covers thread");
    rx
}

fn decode_on_workers(
    agent: ureq::Agent,
    urls: Vec<String>,
    tx: &Sender<Loaded>,
    held: Arc<AtomicUsize>,
    wake: impl Fn() + Clone + Send + 'static,
) {
    let jobs = Arc::new(Mutex::new(urls.into_iter().enumerate()));
    for n in 0..DECODERS {
        let (agent, jobs, tx, held, wake) = (
            agent.clone(),
            jobs.clone(),
            tx.clone(),
            held.clone(),
            wake.clone(),
        );
        std::thread::Builder::new()
            .name(format!("cover-{n}"))
            .spawn(move || {
                loop {
                    let Some((index, url)) = jobs.lock().unwrap().next() else {
                        break;
                    };
                    let loaded = match cover(&agent, &url) {
                        Ok(image) => {
                            held.fetch_add(image.pixels.len() * 4, Ordering::SeqCst);
                            Loaded::Cover(index, image)
                        }
                        Err(e) => Loaded::Failed(format!("cover {index}: {e:#}")),
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
    held: Arc<AtomicUsize>,
    failed: Option<String>,
    #[cfg(feature = "gate-sabotage")]
    kept: Vec<ColorImage>,
}

impl Grid {
    pub fn start(&mut self, ctx: &egui::Context, source: Source) {
        let px = px(COVER_PT, ctx.pixels_per_point());
        let waker = ctx.clone();
        let held = self.held.clone();
        self.rx = Some(fetch(source, px, held, move || waker.request_repaint()));
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
                        self.held.fetch_sub(bytes, Ordering::SeqCst);
                    }
                }
                Loaded::Failed(why) => {
                    self.failed.get_or_insert(why);
                }
            }
        }
        any
    }

    pub fn uploaded(&self) -> usize {
        self.textures.len()
    }

    pub fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
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
