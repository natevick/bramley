use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

pub const ART_CAP: u64 = 512 << 20;
pub const API_CAP: u64 = 64 << 20;

pub static ART_HITS: AtomicU64 = AtomicU64::new(0);
pub static ART_MISSES: AtomicU64 = AtomicU64::new(0);
pub static API_HITS: AtomicU64 = AtomicU64::new(0);
pub static API_MISSES: AtomicU64 = AtomicU64::new(0);
pub static API_REVALIDATIONS: AtomicU64 = AtomicU64::new(0);

pub fn counts() -> serde_json::Value {
    let read = |counter: &AtomicU64| counter.load(SeqCst);
    serde_json::json!({
        "artHits": read(&ART_HITS), "artMisses": read(&ART_MISSES),
        "apiHits": read(&API_HITS), "apiMisses": read(&API_MISSES),
        "apiRevalidations": read(&API_REVALIDATIONS),
    })
}

pub fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"));
    let xdg = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    let base = xdg.filter(|p| p.is_absolute()).or(home);
    base.map(|b| b.join("bramley"))
}

pub fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let unique = format!(
        "{}-{}.partial",
        std::process::id(),
        WRITES.fetch_add(1, SeqCst)
    );
    let partial = path.with_extension(unique);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&partial)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    file.write_all(contents)?;
    file.sync_all()?;
    std::fs::rename(&partial, path)
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

fn files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let stat = |e: std::fs::DirEntry| {
        let meta = e.metadata().ok().filter(|m| m.is_file())?;
        Some((e.path(), meta.len(), meta.modified().ok()?))
    };
    entries.filter_map(stat).collect()
}

pub fn digest(parts: &[&str]) -> String {
    let mut sha = Sha256::new();
    for part in parts {
        sha.update(part.as_bytes());
        sha.update([0]);
    }
    sha.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct State {
    epoch: u64,
    bytes: u64,
}

pub struct Store {
    dir: PathBuf,
    cap: u64,
    state: Mutex<State>,
}

impl Store {
    pub fn open(root: &Path, name: &str, cap: u64) -> std::io::Result<Store> {
        let dir = root.join(name);
        private_dir(root)?;
        private_dir(&dir)?;
        let bytes = files(&dir).iter().map(|f| f.1).sum();
        let state = Mutex::new(State { epoch: 0, bytes });
        Ok(Store { dir, cap, state })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn epoch(&self) -> u64 {
        self.state().epoch
    }

    pub fn path(&self, parts: &[&str]) -> PathBuf {
        self.dir.join(digest(parts))
    }

    pub fn get(&self, parts: &[&str]) -> Option<Vec<u8>> {
        let mut options = std::fs::OpenOptions::new();
        let mut file = options.read(true).write(true).open(self.path(parts)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let _ = file.set_modified(SystemTime::now());
        Some(bytes)
    }

    pub fn put(&self, parts: &[&str], bytes: &[u8], epoch: u64) {
        let mut state = self.state();
        if state.epoch != epoch || write_private(&self.path(parts), bytes).is_err() {
            return;
        }
        state.bytes += bytes.len() as u64;
        if state.bytes > self.cap {
            state.bytes = self.evict();
        }
    }

    fn evict(&self) -> u64 {
        let mut files = files(&self.dir);
        files.sort_by_key(|f| f.2);
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        for (path, len, _) in files {
            if total <= self.cap {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
            }
        }
        total
    }

    pub fn storefront(&self, user: &str) -> Option<String> {
        let found = self.get(&["storefront", &digest(&[user])])?;
        String::from_utf8(found).ok()
    }

    pub fn remember_storefront(&self, user: &str, storefront: &str, epoch: u64) {
        self.put(
            &["storefront", &digest(&[user])],
            storefront.as_bytes(),
            epoch,
        );
    }

    pub fn scope(&self, user: &str) -> String {
        digest(&[user, &self.storefront(user).unwrap_or_default()])
    }

    pub fn wipe(&self) {
        let mut state = self.state();
        state.epoch += 1;
        state.bytes = 0;
        for (path, ..) in files(&self.dir) {
            let _ = std::fs::remove_file(path);
        }
    }
}
