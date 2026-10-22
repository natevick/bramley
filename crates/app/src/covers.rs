use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering::SeqCst as Seq;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
pub use bramley_api::art_url;
use bramley_api::store::{API_CAP, ART_CAP, ART_HITS, ART_MISSES, Store};
use bramley_api::warm::Limiter;
use bramley_engine::{STOREFRONT_BACKOFF, STOREFRONT_DEADLINE, redact, user_agent};
use egui::{ColorImage, TextureHandle, TextureOptions};
use serde_json::Value;

use crate::ui::nav::{Nav, Route};

pub const COVER_PT: f32 = 96.0;
pub const HOME_GAP_PT: f32 = 8.0;
pub const HOME_CELL_MIN_PT: f32 = 56.0;
pub const HOME_CELL_MAX_PT: f32 = 200.0;
const HOME_DECODE_CAP_PX: u32 = 256;
const HOME_CAPTION: &str = "From your library, ranked by what you've been playing.";
pub const API_BASE: &str = "https://api.music.apple.com";
const STOREFRONT: &str = "/v1/me/storefront";
const LIBRARY_ALBUMS: &str = "/v1/me/library/albums?limit=100";
const LOOKUP: usize = 100;
const LIBRARY_PAGE: usize = 100;
const LIBRARY_PAGES: usize = 10;
const REPLAY: &str = "/v1/me/music-summaries?filter%5Byear%5D=latest&views=top-albums,top-songs";
const LISTENING: [&str; 3] = [
    "/v1/me/history/heavy-rotation",
    "/v1/me/recent/played",
    "/v1/me/library/recently-added",
];
const ART_HOST: &str = ".mzstatic.com";
const DECODERS: usize = 2;
const COUNT: usize = 50;
const MAX_BODY_BYTES: u64 = 4 << 20;
const RECEIVE_RESPONSE: Duration = Duration::from_secs(10);
const BACKOFF: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

pub const DEVELOPER_REJECTED: &str = "Bramley's developer token was rejected";

#[derive(Debug)]
struct DeveloperTokenRejected;

impl std::fmt::Display for DeveloperTokenRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(DEVELOPER_REJECTED)
    }
}

impl std::error::Error for DeveloperTokenRejected {}

#[derive(Debug)]
pub(crate) struct Refused {
    pub(crate) status: u16,
    pub(crate) wait: Duration,
    why: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

impl std::error::Error for Refused {}

type Round = u8;
const SHOWN: Round = 0;
const FRESH: Round = 1;

pub enum Loaded {
    Cover(Round, usize, String, ColorImage),
    Missed(Round, String),
    Failed(String, usize),
    Live(usize),
    Total(usize),
    Retry(Duration),
    SignInNeeded,
    DeveloperRejected(String),
    Empty,
}

#[derive(Default)]
pub struct Tally {
    in_flight: AtomicUsize,
    requested: AtomicUsize,
    cancel: AtomicBool,
    art: Option<Arc<Store>>,
    limiter: Option<Arc<Limiter>>,
}

impl Tally {
    pub fn caching(art: Option<Arc<Store>>) -> Self {
        Tally {
            art,
            ..Tally::default()
        }
    }

    pub fn warming(art: Arc<Store>, limiter: Arc<Limiter>) -> Self {
        Tally {
            limiter: Some(limiter),
            ..Tally::caching(Some(art))
        }
    }
}

#[derive(Clone)]
pub struct Source {
    pub api: String,
    pub art_prefix: Option<String>,
    pub user: crate::credentials::Credentials,
    pub art_cache: Option<Arc<Store>>,
    pub api_cache: Option<Arc<Store>>,
}

pub fn open_caches() -> (Option<Arc<Store>>, Option<Arc<Store>>) {
    let open = |root: &std::path::Path, name, cap| {
        let opened = Store::open(root, name, cap);
        opened
            .inspect_err(|e| eprintln!("bramley: the {name} cache is off: {e}"))
            .ok()
            .map(Arc::new)
    };
    match bramley_api::store::cache_dir() {
        Some(root) => (open(&root, "art", ART_CAP), open(&root, "api", API_CAP)),
        None => (None, None),
    }
}

pub fn px(points: f32, pixels_per_point: f32) -> u32 {
    (points * pixels_per_point).round() as u32
}

pub fn home_px(pixels_per_point: f32) -> u32 {
    px(HOME_CELL_MAX_PT, pixels_per_point).min(HOME_DECODE_CAP_PX)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomeLayout {
    pub columns: usize,
    pub cell: f32,
    pub shown: usize,
}

fn cell_across(width: f32, columns: usize) -> f32 {
    let gaps = HOME_GAP_PT * (columns - 1) as f32;
    ((width - gaps) / columns as f32 * 64.0).floor().max(0.0) / 64.0
}

fn width_columns(width: f32) -> usize {
    (((width + HOME_GAP_PT) / (COVER_PT + HOME_GAP_PT)) as usize).max(1)
}

pub fn home_layout(width: f32, count: usize) -> HomeLayout {
    let target = |cell: f32| (cell / COVER_PT).ln().abs();
    let exact = (2..count)
        .filter(|columns| count.is_multiple_of(*columns))
        .map(|columns| (columns, cell_across(width, columns)))
        .filter(|(_, cell)| (HOME_CELL_MIN_PT..=HOME_CELL_MAX_PT).contains(cell))
        .min_by(|a, b| target(a.1).total_cmp(&target(b.1)));
    if let Some((columns, cell)) = exact {
        return HomeLayout {
            columns,
            cell,
            shown: count,
        };
    }
    let columns = width_columns(width);
    HomeLayout {
        columns,
        cell: cell_across(width, columns).min(HOME_CELL_MAX_PT),
        shown: if count >= columns {
            count / columns * columns
        } else {
            count
        },
    }
}

pub fn home_heading(shown: usize) -> String {
    let plural = if shown == 1 { "" } else { "s" };
    format!("Top {shown} album{plural}")
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

pub(crate) fn agent() -> ureq::Agent {
    configured(10)
}

fn configured(idle: usize) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(user_agent())
        .timeout_global(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(RECEIVE_RESPONSE))
        .max_idle_connections(idle)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(
    agent: &mut ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    tally: &Tally,
) -> Result<(u16, Vec<u8>, Duration)> {
    if tally.cancel.load(Ordering::SeqCst) {
        bail!("cancelled");
    }
    tally.requested.fetch_add(1, Ordering::SeqCst);
    let send = |agent: &ureq::Agent| {
        let request = headers
            .iter()
            .fold(agent.get(url), |r, (k, v)| r.header(*k, *v));
        request.call()
    };
    let mut response = send(agent).or_else(|first| {
        *agent = self::agent();
        if tally
            .limiter
            .as_ref()
            .is_some_and(|l| l.spend(false).is_err())
        {
            return Err(first);
        }
        send(&configured(0))
    })?;
    let status = response.status().as_u16();
    let wait = bramley_api::retry_after(response.headers());
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()?;
    Ok((status, body, wait))
}

fn apple_error((status, body, wait): (u16, Vec<u8>, Duration)) -> anyhow::Error {
    let body: Value = serde_json::from_slice(&body).unwrap_or_default();
    let detail = &body["errors"][0];
    let why = match detail["detail"].as_str().or(detail["title"].as_str()) {
        Some(why) => format!("HTTP {status}: {why}"),
        None => format!("HTTP {status}"),
    };
    anyhow!(Refused { status, wait, why })
}

fn signed_in(
    agent: &mut ureq::Agent,
    source: &Source,
    bearer: &str,
    user: &str,
    tally: &Tally,
) -> Result<Option<Vec<u8>>> {
    let headers = [("Authorization", bearer), ("Music-User-Token", user)];
    let url = format!("{}{STOREFRONT}", source.api);
    let until = Instant::now() + STOREFRONT_DEADLINE;
    let mut waits = STOREFRONT_BACKOFF.into_iter();
    loop {
        match get(agent, &url, &headers, tally).context("sign-in check")? {
            (200, body, _) => return Ok(Some(body)),
            (401, ..) => return Err(anyhow!(DeveloperTokenRejected)),
            (403, ..) => match waits.next().filter(|wait| Instant::now() + *wait < until) {
                Some(wait) => std::thread::sleep(wait),
                None => return Ok(None),
            },
            refused => return Err(apple_error(refused).context("sign-in check")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Album {
    pub(crate) id: String,
    pub(crate) art: Option<String>,
    pub(crate) edition: String,
}

#[derive(Clone)]
pub(crate) enum Pick {
    Library(Album),
    Catalog(String),
}

pub(crate) fn rank(
    sources: &[Vec<Pick>],
    found: &HashMap<String, Album>,
    library: Vec<Album>,
) -> Vec<Album> {
    let picked = sources.iter().flatten().filter_map(|pick| match pick {
        Pick::Library(album) => Some(album.clone()),
        Pick::Catalog(id) => found.get(id).cloned(),
    });
    let (mut ids, mut arts, mut editions) = (HashSet::new(), HashSet::new(), HashSet::new());
    let unseen = |a: &Album| {
        ids.insert(a.id.clone())
            && a.art.iter().all(|url| arts.insert(url.clone()))
            && editions.insert(a.edition.clone())
    };
    picked.chain(library).filter(unseen).collect()
}

fn pick(resource: &Value) -> Option<Pick> {
    let id = resource["id"].as_str()?;
    match resource["type"].as_str()? {
        "library-albums" => album(resource).map(Pick::Library),
        "albums" if crate::cli::catalog_id(id).is_ok() => Some(Pick::Catalog(id.to_owned())),
        _ => None,
    }
}

fn picks(items: &Value) -> Vec<Pick> {
    let items = items.as_array().map(Vec::as_slice).unwrap_or_default();
    items.iter().filter_map(pick).collect()
}

fn related<'a>(items: &'a Value, relationship: &str) -> Vec<&'a Value> {
    let items = items.as_array().map(Vec::as_slice).unwrap_or_default();
    let first = |i: &'a Value| i["relationships"][relationship]["data"].get(0);
    items.iter().filter_map(first).collect()
}

fn album(resource: &Value) -> Option<Album> {
    let id = resource["id"]
        .as_str()
        .filter(|id| bramley_api::segment(id))?;
    let attributes = &resource["attributes"];
    let (artist, name) = (
        attributes["artistName"].as_str(),
        attributes["name"].as_str(),
    );
    Some(Album {
        id: id.to_owned(),
        art: attributes["artwork"]["url"].as_str().map(str::to_owned),
        edition: artist
            .zip(name)
            .and_then(|(artist, name)| edition(artist, name))
            .unwrap_or_else(|| id.to_owned()),
    })
}

const EDITION_WORDS: [&str; 11] = [
    "remaster",
    "remastered",
    "deluxe",
    "expanded",
    "anniversary",
    "edition",
    "mono",
    "stereo",
    "bonus",
    "special",
    "mix",
];

fn qualifier(span: &str) -> bool {
    let span = span.to_lowercase();
    let mut words = span.split(|c: char| !c.is_alphanumeric());
    words.any(|word| EDITION_WORDS.contains(&word))
}

pub(crate) fn edition(artist: &str, name: &str) -> Option<String> {
    let (mut title, mut rest) = (String::new(), name);
    while let Some(open) = rest.find(['(', '[']) {
        let Some(close) = rest[open..].find([')', ']']).map(|at| open + at) else {
            break;
        };
        title.push_str(&rest[..open]);
        if !qualifier(&rest[open + 1..close]) {
            title.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    title.push_str(rest);
    let title: Vec<&str> = title.split_whitespace().collect();
    let artist = artist.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then(|| format!("{artist}\0{}", title.join(" ")).to_lowercase())
}

fn storefront(body: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(body).ok()?;
    let id = body["data"][0]["id"].as_str()?;
    bramley_api::segment(id).then(|| id.to_owned())
}

fn ok_body(
    agent: &mut ureq::Agent,
    (path, headers): (&str, &[(&str, &str)]),
    source: &Source,
    tally: &Tally,
) -> Result<Option<Value>> {
    match get(agent, &format!("{}{path}", source.api), headers, tally) {
        Ok((200, body, _)) => Ok(serde_json::from_slice(&body).ok()),
        Ok((401, ..)) => Err(anyhow!(DeveloperTokenRejected)),
        _ => Ok(None),
    }
}

fn replay(
    agent: &mut ureq::Agent,
    (source, bearer, user): (&Source, &str, &str),
    (storefront, early): (Option<String>, Sender<Vec<Pick>>),
    tally: &Tally,
) -> Result<Vec<Vec<Pick>>> {
    let me = [("Authorization", bearer), ("Music-User-Token", user)];
    let found = ok_body(agent, (REPLAY, &me), source, tally)?;
    let view = |name: &str| {
        found
            .as_ref()
            .map_or(Value::Null, |r| r["data"][0]["views"][name]["data"].clone())
    };
    let (top_albums, top_songs) = (view("top-albums"), view("top-songs"));
    let albums = |found: &Value, relationship| -> Vec<Pick> {
        related(found, relationship)
            .into_iter()
            .filter_map(pick)
            .collect()
    };
    let top = albums(&top_albums, "album");
    drop(early.send(top.clone()));
    let ids: Vec<&str> = related(&top_songs, "song")
        .iter()
        .filter_map(|song| song["id"].as_str())
        .filter(|id| crate::cli::catalog_id(id).is_ok())
        .collect();
    let songs = match storefront.filter(|_| !ids.is_empty()) {
        Some(sf) => {
            let path = format!(
                "/v1/catalog/{sf}/songs?include=albums&ids={}",
                ids.join(",")
            );
            ok_body(agent, (&path, &[("Authorization", bearer)]), source, tally)?
        }
        None => None,
    };
    Ok(vec![
        top,
        songs.map_or(Vec::new(), |s| albums(&s["data"], "albums")),
    ])
}

fn listed(
    agent: &mut ureq::Agent,
    (source, bearer, user): (&Source, &str, &str),
    (path, tally): (&str, &Tally),
) -> Result<Vec<Pick>> {
    let me = [("Authorization", bearer), ("Music-User-Token", user)];
    let found = ok_body(agent, (path, &me), source, tally)?;
    Ok(found.map_or(Vec::new(), |f| picks(&f["data"])))
}

fn lookup(
    agent: &mut ureq::Agent,
    (source, bearer, user): (&Source, &str, &str),
    (storefront, sources): (Option<&str>, &[Vec<Pick>]),
    tally: &Tally,
) -> Result<HashMap<String, Album>> {
    let catalog = sources.iter().flatten().filter_map(|pick| match pick {
        Pick::Catalog(id) => Some(id.as_str()),
        Pick::Library(_) => None,
    });
    let mut seen = HashSet::new();
    let ids: Vec<&str> = catalog.filter(|id| seen.insert(*id)).take(LOOKUP).collect();
    let Some(storefront) = storefront.filter(|_| !ids.is_empty()) else {
        return Ok(HashMap::new());
    };
    let path = format!(
        "/v1/catalog/{storefront}/albums?include=library&ids={}",
        ids.join(",")
    );
    let me = [("Authorization", bearer), ("Music-User-Token", user)];
    let found = ok_body(agent, (&path, &me), source, tally)?.unwrap_or_default();
    let items = found["data"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let owned = |item: &Value| {
        let library = item["relationships"]["library"]["data"].get(0)?;
        Some((item["id"].as_str()?.to_owned(), album(library)?))
    };
    Ok(items.iter().filter_map(owned).collect())
}

fn library_albums(
    agent: &mut ureq::Agent,
    (source, bearer, user): (&Source, &str, &str),
    tally: &Tally,
) -> Result<Vec<Album>> {
    let headers = [("Authorization", bearer), ("Music-User-Token", user)];
    let mut albums = Vec::new();
    for page in 0..LIBRARY_PAGES {
        let path = format!("{LIBRARY_ALBUMS}&offset={}", page * LIBRARY_PAGE);
        let (status, body, wait) = get(agent, &format!("{}{path}", source.api), &headers, tally)
            .context("library albums")?;
        match status {
            200 => {}
            401 => return Err(anyhow!(DeveloperTokenRejected)),
            _ => return Err(apple_error((status, body, wait)).context("library albums")),
        }
        let body: Value = serde_json::from_slice(&body).context("library albums")?;
        let data = body["data"]
            .as_array()
            .context("the library has no album list")?;
        albums.extend(data.iter().filter_map(album));
        if body["next"].is_null() || albums.len() >= 2 * COUNT {
            break;
        }
    }
    Ok(albums)
}

fn joined<T>(handle: std::thread::ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    handle
        .join()
        .unwrap_or_else(|_| Err(anyhow!("a home request panicked")))
}

#[derive(Default)]
struct Sent {
    emitted: usize,
    processed: usize,
    skipped: usize,
    grid: Vec<(String, String)>,
}

fn send(
    ranked: Vec<Album>,
    sent: &mut Sent,
    (jobs, px, art_prefix): (&Sender<Job>, u32, Option<&str>),
) {
    for album in ranked.into_iter().skip(sent.processed) {
        if sent.emitted == COUNT {
            break;
        }
        sent.processed += 1;
        let art = album.art.as_deref();
        let url = art.and_then(|art| art_url(art, px));
        match url.filter(|url| allowed_art(url, art_prefix)).zip(art) {
            Some((url, art)) => {
                sent.grid.push((album.id.clone(), art.to_owned()));
                drop(jobs.send((SHOWN, sent.emitted, album.id, url)));
                sent.emitted += 1;
            }
            None => sent.skipped += 1,
        }
    }
}

fn home(
    agent: &mut ureq::Agent,
    auth: (&Source, &str, &str),
    (signed_in, px): (&[u8], u32),
    (jobs, tally): (&Sender<Job>, &Tally),
) -> Result<Sent> {
    let storefront = storefront(signed_in);
    let out = (jobs, px, auth.0.art_prefix.as_deref());
    let (mut sent, mut found) = (Sent::default(), HashMap::new());
    std::thread::scope(|scope| {
        let agent = &*agent;
        let (early, first) = channel();
        let sf = (storefront.clone(), early);
        let replay = scope.spawn(|| replay(&mut agent.clone(), auth, sf, tally));
        let listened = LISTENING
            .map(|path| scope.spawn(move || listed(&mut agent.clone(), auth, (path, tally))));
        let library = scope.spawn(|| library_albums(&mut agent.clone(), auth, tally));
        if let Ok(top) = first.recv() {
            let top = [top];
            let early = lookup(
                &mut agent.clone(),
                auth,
                (storefront.as_deref(), &top),
                tally,
            )?;
            send(rank(&top, &early, Vec::new()), &mut sent, out);
            found = early;
        }
        let sources = listened.into_iter().fold(joined(replay), |all, found| {
            let (mut all, found) = (all?, joined(found)?);
            all.push(found);
            Ok::<_, anyhow::Error>(all)
        })?;
        found.extend(lookup(
            &mut agent.clone(),
            auth,
            (storefront.as_deref(), &sources),
            tally,
        )?);
        send(rank(&sources, &found, Vec::new()), &mut sent, out);
        if sent.emitted < COUNT {
            send(rank(&sources, &found, joined(library)?), &mut sent, out);
        }
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(sent)
}

fn home_cached(
    agent: &mut ureq::Agent,
    auth: (&Source, &str, &str),
    (signed_in, px): (&[u8], u32),
    (jobs, tx, tally): (&Sender<Job>, &Sender<Loaded>, &Tally),
) -> Result<(usize, usize)> {
    let (source, _, user) = auth;
    let Some(store) = source.api_cache.as_deref() else {
        let live = home(agent, auth, (signed_in, px), (jobs, tally))?;
        return Ok((live.emitted, live.skipped));
    };
    let (epoch, cached) = (store.epoch(), store.home(user));
    let (held, waiting) = channel();
    let target = if cached.is_some() { &held } else { jobs };
    let remembered = cached.iter().flatten().map(|(id, art)| Album {
        id: id.clone(),
        art: Some(art.clone()),
        edition: String::new(),
    });
    let prefix = source.art_prefix.as_deref();
    let mut kept = Sent::default();
    send(remembered.collect(), &mut kept, (jobs, px, prefix));
    if cached.is_some() {
        drop(tx.send(Loaded::Total(kept.emitted)));
    }
    let live = home(agent, auth, (signed_in, px), (target, tally))?;
    if cached.as_ref() != Some(&live.grid) {
        store.remember_home(user, &live.grid, epoch);
        if cached.is_some() {
            for (_, index, id, url) in waiting.try_iter() {
                drop(jobs.send((FRESH, index, id, url)));
            }
            drop(tx.send(Loaded::Live(live.emitted)));
        }
    }
    Ok((live.emitted, live.skipped))
}

pub(crate) fn art_key(url: &str) -> [&str; 2] {
    ["art", url]
}

pub(crate) fn cover(
    agent: &mut ureq::Agent,
    url: &str,
    px: u32,
    tally: &Tally,
) -> Result<ColorImage> {
    let key = art_key(url);
    let store = tally.art.as_deref();
    let kept = store
        .and_then(|s| s.get(&key))
        .and_then(|b| decode(&b, px).ok());
    if let Some(image) = kept {
        ART_HITS.fetch_add(1, Seq);
        return Ok(image);
    }
    ART_MISSES.fetch_add(1, Seq);
    let (status, bytes, wait) = get(agent, url, &[], tally)?;
    match status {
        200 => {}
        300..=399 => bail!("redirected (HTTP {status}); redirects are not followed"),
        _ => {
            let why = format!("HTTP {status}");
            return Err(anyhow!(Refused { status, wait, why }));
        }
    }
    let image = decode(&bytes, px)?;
    if let Some(store) = store {
        store.put(&key, &bytes, store.epoch());
    }
    Ok(image)
}

fn decode(bytes: &[u8], px: u32) -> Result<ColorImage> {
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

type Job = (Round, usize, String, String);

fn load_home(
    agent: &mut ureq::Agent,
    source: &Source,
    px: u32,
    (jobs, tx, tally): (&Sender<Job>, &Sender<Loaded>, &Tally),
) -> Result<Option<(usize, usize)>> {
    let tokens = source.user.developer();
    let cancelled = || tally.cancel.load(Ordering::SeqCst);
    let developer = tokens.wait(cancelled).map_err(|why| anyhow!(why))?;
    let Some(user) = source.user.load().map_err(|why| anyhow!(why))? else {
        return Ok(None);
    };
    let mut attempt = |developer: &str| {
        let bearer = format!("Bearer {developer}");
        let checked = signed_in(agent, source, &bearer, &user, tally).and_then(|body| {
            let auth = (source, bearer.as_str(), user.as_str());
            body.map(|body| home_cached(agent, auth, (&body, px), (jobs, tx, tally)))
                .transpose()
        });
        checked.map_err(|e| {
            if e.is::<DeveloperTokenRejected>() {
                return e;
            }
            let why = redact(&format!("{e:#}"), &[developer.into(), user.clone()]);
            match e.downcast_ref::<Refused>() {
                Some(&Refused { status, wait, .. }) => anyhow!(Refused { status, wait, why }),
                None => anyhow!("{why}"),
            }
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
            let mut agent = agent();
            let (jobs, queue) = channel();
            decode_on_workers(agent.clone(), (queue, px), &tx, tally.clone(), wake.clone());
            match load_home(&mut agent, &source, px, (&jobs, &tx, &tally)) {
                Ok(None) => drop(tx.send(Loaded::SignInNeeded)),
                Err(e) if e.is::<DeveloperTokenRejected>() => {
                    let rejected = source.user.developer().peek().unwrap_or_default();
                    drop(tx.send(Loaded::DeveloperRejected(rejected)))
                }
                Err(e) => {
                    let refused = e.downcast_ref::<Refused>();
                    if refused.is_none_or(|r| r.status != 403) {
                        let wait = refused.map_or(Duration::ZERO, |r| r.wait);
                        let _ = tx.send(Loaded::Retry(wait));
                    }
                    let _ = tx.send(Loaded::Failed(format!("{e:#}"), COUNT));
                }
                Ok(Some((0, 0))) => drop(tx.send(Loaded::Empty)),
                Ok(Some((emitted, 0))) => drop(tx.send(Loaded::Total(emitted))),
                Ok(Some((emitted, skipped))) => {
                    let _ = tx.send(Loaded::Total(emitted));
                    let why = format!("{skipped} library albums had no usable artwork");
                    let _ = tx.send(Loaded::Failed(why, skipped));
                }
            }
            wake();
        })
        .expect("spawn covers thread");
    rx
}

fn decode_on_workers(
    agent: ureq::Agent,
    (queue, px): (Receiver<Job>, u32),
    tx: &Sender<Loaded>,
    tally: Arc<Tally>,
    wake: impl Fn() + Clone + Send + 'static,
) {
    let jobs = Arc::new(Mutex::new(queue));
    for n in 0..DECODERS {
        let (mut agent, jobs, tx, tally, wake) = (
            agent.clone(),
            jobs.clone(),
            tx.clone(),
            tally.clone(),
            wake.clone(),
        );
        std::thread::Builder::new()
            .name(format!("cover-{n}"))
            .spawn(move || {
                while let Ok((round, index, id, url)) = jobs.lock().unwrap().recv() {
                    let decoded =
                        catch_unwind(AssertUnwindSafe(|| cover(&mut agent, &url, px, &tally)));
                    let loaded =
                        match decoded.unwrap_or_else(|_| Err(anyhow!("the decoder panicked"))) {
                            Ok(image) => {
                                tally
                                    .in_flight
                                    .fetch_add(image.pixels.len() * 4, Ordering::SeqCst);
                                Loaded::Cover(round, index, id, image)
                            }
                            Err(e) => Loaded::Missed(round, format!("cover {index}: {e:#}")),
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
    textures: BTreeMap<usize, (String, TextureHandle)>,
    generation: Round,
    fresh: BTreeMap<usize, (String, TextureHandle)>,
    expecting: Option<usize>,
    total: Option<usize>,
    missed: usize,
    tally: Arc<Tally>,
    failed: Option<String>,
    failures: usize,
    rejected: bool,
    empty: bool,
    developer_rejected: Option<String>,
    tries: usize,
    retry_at: Option<Instant>,
    #[cfg(feature = "gate-sabotage")]
    kept: Vec<ColorImage>,
}

impl Grid {
    pub fn new(source: Option<Source>) -> Self {
        let art = source.as_ref().and_then(|s| s.art_cache.clone());
        Grid {
            source,
            tally: Arc::new(Tally::caching(art)),
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
                let developer_rejected = self.developer_rejected.take();
                self.reset();
                self.developer_rejected = developer_rejected;
            }
            return;
        }
        let px = home_px(ctx.pixels_per_point());
        let Some(source) = self.source.clone().filter(|_| self.px != px) else {
            return;
        };
        self.start(ctx, source, px, 0);
    }

    pub fn retry(&mut self, ctx: &egui::Context, now: Instant, visible: bool) -> bool {
        let due = self.retry_at.is_some_and(|at| visible && now >= at);
        let source = self.source.clone().filter(|_| due);
        if let Some(source) = &source {
            self.start(ctx, source.clone(), self.px, self.tries);
        }
        source.is_some()
    }

    fn start(&mut self, ctx: &egui::Context, source: Source, px: u32, tries: usize) {
        self.reset();
        (self.px, self.tries) = (px, tries);
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
                Loaded::Cover(round, index, id, image) => {
                    let bytes = image.pixels.len() * 4;
                    #[cfg(feature = "gate-sabotage")]
                    self.kept.push(image.clone());
                    if round >= self.generation {
                        let name = format!("cover-{index}");
                        let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                        let shelf = if round == FRESH {
                            &mut self.fresh
                        } else {
                            &mut self.textures
                        };
                        shelf.insert(index, (id, texture));
                    }
                    self.tally.in_flight.fetch_sub(bytes, Ordering::SeqCst);
                }
                Loaded::Missed(round, why) if round >= self.generation => {
                    self.failed.get_or_insert(why);
                    self.failures += 1;
                    self.missed += usize::from(round == FRESH);
                }
                Loaded::Missed(..) => {}
                Loaded::Live(count) => self.expecting = Some(count),
                Loaded::Total(count) => {
                    if self.expecting.is_none() {
                        self.total = Some(count);
                    }
                }
                Loaded::Failed(why, covers) => {
                    self.failed.get_or_insert(why);
                    self.failures += covers;
                }
                Loaded::Retry(after) => {
                    let wait = BACKOFF[self.tries.min(BACKOFF.len() - 1)].max(after);
                    (self.retry_at, self.tries) = (Some(Instant::now() + wait), self.tries + 1);
                    ctx.request_repaint_after(wait);
                }
                Loaded::SignInNeeded => self.rejected = true,
                Loaded::Empty => self.empty = true,
                Loaded::DeveloperRejected(token) => self.developer_rejected = Some(token),
            }
        }
        if self
            .expecting
            .is_some_and(|n| self.fresh.len() + self.missed >= n)
        {
            self.textures = std::mem::take(&mut self.fresh);
            self.total = Some(self.textures.len());
            (self.generation, self.expecting, self.missed) = (FRESH, None, 0);
        }
        any
    }

    pub fn rejected(&self) -> bool {
        self.rejected
    }

    pub fn rejected_developer_token(&self) -> Option<&str> {
        self.developer_rejected.as_deref()
    }

    #[cfg(any(test, feature = "dev"))]
    pub fn album(&self, n: usize) -> Option<&str> {
        self.textures.values().nth(n).map(|(id, _)| id.as_str())
    }

    #[cfg(test)]
    pub fn listen(&mut self, rx: Receiver<Loaded>) {
        self.rx = Some(rx);
    }

    #[cfg(test)]
    pub fn empty(&self) -> bool {
        self.empty
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

    pub fn show(&self, ui: &mut egui::Ui, navs: &mut Vec<Nav>) {
        if let Some(why) = &self.failed {
            ui.label(egui::RichText::new(format!("Some covers did not load: {why}")).weak());
        }
        if self.empty {
            ui.label(egui::RichText::new("Your library has no albums yet.").weak());
        }
        let settled = self
            .total
            .is_some_and(|total| self.textures.len() + self.failures >= total);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let width = ui.available_width();
            let layout = if settled {
                home_layout(width, self.textures.len())
            } else {
                HomeLayout {
                    columns: width_columns(width),
                    cell: COVER_PT,
                    shown: self.textures.len(),
                }
            };
            if settled && layout.shown > 0 {
                let title = home_heading(layout.shown);
                ui.label(egui::RichText::new(&title).strong().size(16.0));
                ui.label(egui::RichText::new(HOME_CAPTION).weak().size(12.0));
            }
            ui.spacing_mut().item_spacing = egui::vec2(HOME_GAP_PT, HOME_GAP_PT);
            let size = egui::vec2(layout.cell, layout.cell);
            let shown: Vec<_> = self
                .textures
                .values()
                .take(layout.shown)
                .enumerate()
                .collect();
            for row in shown.chunks(layout.columns) {
                ui.horizontal(|ui| {
                    for &(n, (id, texture)) in row {
                        let image = egui::Image::new(texture).fit_to_exact_size(size);
                        let cover = ui.add(egui::Button::image(image).frame(false));
                        let label = format!("Open album {}", n + 1);
                        let cover = crate::ui::labeled(cover, egui::WidgetType::Button, &label);
                        if cover.clicked() {
                            navs.push(Nav::Go(Route::Album(id.clone())));
                        }
                    }
                });
            }
        });
    }
}
