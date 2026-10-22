use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use keyring_core::api::CredentialStore;
use keyring_core::{Entry, Error};

use crate::devtoken::DeveloperToken;

pub const USER_TOKEN: &str = "music-user-token";
const DEFAULT_SERVICE: &str = "bramley";
const OPEN_WAIT: Duration = Duration::from_secs(5);
const WAIT: Duration = Duration::from_secs(60);
const UNAVAILABLE: &str = "the system keyring did not answer; unlock it and try again";
const STALE: &str = "Bramley signed out while this sign-in was arriving";
const NO_NATIVE_SIGN_IN: &str = "this platform signs in through the browser";
const SIGNED_BY_MACOS: &str = "macOS holds the Apple Music sign-in";

#[derive(Clone)]
enum Backend {
    Keyring(Arc<CredentialStore>),
    #[cfg(any(test, not(target_os = "macos")))]
    File,
}

impl Backend {
    fn name(&self) -> &'static str {
        match self {
            Backend::Keyring(_) => "keyring",
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::File => "file",
        }
    }
}

#[derive(Clone)]
pub struct Credentials {
    state: Arc<Mutex<Option<Backend>>>,
    mode: Arc<OnceLock<&'static str>>,
    service: String,
    dir: PathBuf,
    developer: DeveloperToken,
}

impl Credentials {
    pub fn system(dir: &Path, developer: DeveloperToken) -> Self {
        let service = std::env::var("BRAMLEY_KEYRING_SERVICE")
            .ok()
            .filter(|s| !s.is_empty());
        let service = service.as_deref().unwrap_or(DEFAULT_SERVICE);
        Credentials::with(None, service, dir, developer)
    }

    #[cfg(test)]
    pub fn new(store: Option<Arc<CredentialStore>>, service: &str, dir: &Path) -> Self {
        Credentials::with(
            Some(store.map_or(Backend::File, Backend::Keyring)),
            service,
            dir,
            DeveloperToken::new(None, dir, dir),
        )
    }

    #[cfg(test)]
    pub fn with_developer(mut self, developer: DeveloperToken) -> Self {
        self.developer = developer;
        self
    }

    fn with(
        backend: Option<Backend>,
        service: &str,
        dir: &Path,
        developer: DeveloperToken,
    ) -> Self {
        let mode = OnceLock::new();
        if let Some(backend) = &backend {
            let _ = mode.set(backend.name());
        }
        Credentials {
            state: Arc::new(Mutex::new(backend)),
            mode: Arc::new(mode),
            service: service.to_owned(),
            dir: dir.to_owned(),
            developer,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn developer(&self) -> &DeveloperToken {
        &self.developer
    }

    pub fn storage(&self) -> Option<&'static str> {
        let native = self.developer.musickit().map(|_| "musickit");
        native.or_else(|| self.mode.get().copied())
    }

    pub fn native(&self) -> bool {
        self.developer.musickit().is_some()
    }

    pub fn signed_in(&self) -> Result<bool, &'static str> {
        match self.developer.musickit() {
            Some(session) => session.authorized(),
            None => self.load().map(|token| token.is_some()),
        }
    }

    pub fn request(&self) -> Result<bool, &'static str> {
        match self.developer.musickit() {
            Some(session) => session.request(),
            None => Err(NO_NATIVE_SIGN_IN),
        }
    }

    fn locked<T>(
        &self,
        op: impl FnOnce(&Backend) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let backend = match state.clone() {
            Some(backend) => backend,
            None => {
                let backend = bounded(OPEN_WAIT, open_native)
                    .flatten()
                    .ok_or(UNAVAILABLE)?;
                let _ = self.mode.set(backend.name());
                state.insert(backend).clone()
            }
        };
        op(&backend)
    }

    fn entry<T: Send + 'static>(
        &self,
        store: &Arc<CredentialStore>,
        op: impl FnOnce(&Entry) -> keyring_core::Result<T> + Send + 'static,
    ) -> Result<keyring_core::Result<T>, &'static str> {
        let (store, service) = (store.clone(), self.service.clone());
        bounded(WAIT, move || {
            store.build(&service, USER_TOKEN, None).and_then(|e| op(&e))
        })
        .ok_or(UNAVAILABLE)
    }

    fn keep_in(&self, store: &Arc<CredentialStore>, token: &str) -> Result<(), &'static str> {
        let copy = token.to_owned();
        let stored = self.entry(store, move |e| {
            e.set_password(&copy).and_then(|()| e.get_password())
        })?;
        if stored.ok().as_deref() != Some(token) {
            return Err(UNAVAILABLE);
        }
        remove(&self.dir.join(USER_TOKEN))
    }

    pub fn load(&self) -> Result<Option<String>, &'static str> {
        if let Some(session) = self.developer.musickit() {
            return session.user();
        }
        let file = || crate::read_token(&self.dir, USER_TOKEN).ok();
        self.locked(|backend| match backend {
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::File => Ok(file()),
            Backend::Keyring(store) => match self.entry(store, Entry::get_password)? {
                Ok(token) => Ok(Some(token.trim().to_owned()).filter(|t| !t.is_empty())),
                Err(Error::NoEntry) => match file() {
                    Some(token) => self.keep_in(store, &token).map(|()| Some(token)),
                    None => Ok(None),
                },
                Err(_) => Err(UNAVAILABLE),
            },
        })
    }

    pub fn save(&self, token: &str, current: impl Fn() -> bool) -> Result<(), &'static str> {
        if self.native() {
            return Err(SIGNED_BY_MACOS);
        }
        self.locked(|backend| match backend {
            _ if !current() => Err(STALE),
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::File => bramley_api::store::write_private(
                &self.dir.join(USER_TOKEN),
                format!("{token}\n").as_bytes(),
            )
            .map_err(|_| "the sign-in file could not be written"),
            Backend::Keyring(store) => self.keep_in(store, token),
        })
    }

    pub fn clear(&self) -> Result<(), &'static str> {
        if let Some(session) = self.developer.musickit() {
            session.forget();
            return Ok(());
        }
        self.locked(|backend| {
            if let Backend::Keyring(store) = backend
                && !matches!(
                    self.entry(store, Entry::delete_credential)?,
                    Ok(()) | Err(Error::NoEntry)
                )
            {
                return Err(UNAVAILABLE);
            }
            remove(&self.dir.join(USER_TOKEN))
        })
    }
}

#[cfg(target_os = "linux")]
pub fn keyring_locked() -> bool {
    let collection = || {
        let locked = || -> zbus::Result<bool> {
            let bus = zbus::blocking::Connection::session()?;
            let collection = zbus::blocking::Proxy::new(
                &bus,
                "org.freedesktop.secrets",
                "/org/freedesktop/secrets/aliases/default",
                "org.freedesktop.Secret.Collection",
            )?;
            collection.get_property("Locked")
        };
        bounded(OPEN_WAIT, locked).and_then(Result::ok)
    };
    locked_by(
        bounded(OPEN_WAIT, secret_service_on_bus).flatten(),
        collection,
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn locked_by(on_bus: Option<bool>, collection: impl FnOnce() -> Option<bool>) -> bool {
    match on_bus {
        Some(false) => false,
        Some(true) => collection().unwrap_or(true),
        None => true,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn keyring_locked() -> bool {
    false
}

fn remove(path: &Path) -> Result<(), &'static str> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err("the old sign-in file could not be removed")
        }
        _ => Ok(()),
    }
}

fn bounded<T: Send + 'static>(
    wait: Duration,
    job: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("keyring".into())
        .spawn(move || drop(tx.send(job())))
        .ok()?;
    rx.recv_timeout(wait).ok()
}

fn open_native() -> Option<Backend> {
    #[cfg(target_os = "linux")]
    let store = match secret_service_on_bus()? {
        false => return Some(Backend::File),
        true => zbus_secret_service_keyring_store::Store::new(),
    };
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new();
    #[cfg(windows)]
    let store = windows_native_keyring_store::Store::new();
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    return Some(Backend::Keyring(store.ok()?));
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Some(Backend::File)
}

#[cfg(target_os = "linux")]
fn secret_service_on_bus() -> Option<bool> {
    const NAME: &str = "org.freedesktop.secrets";
    let Ok(bus) = zbus::blocking::Connection::session() else {
        return Some(false);
    };
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).ok()?;
    if dbus.name_has_owner(NAME.try_into().ok()?).ok()? {
        return Some(true);
    }
    let activatable = dbus.list_activatable_names().ok()?;
    Some(activatable.iter().any(|name| name.as_str() == NAME))
}
