use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use keyring_core::api::CredentialStore;
use keyring_core::{Entry, Error};

pub const USER_TOKEN: &str = "music-user-token";
const DEFAULT_SERVICE: &str = "bramley";
const QUICK: Duration = Duration::from_secs(5);
const PATIENT: Duration = Duration::from_secs(60);

static WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct Credentials {
    store: Option<Arc<CredentialStore>>,
    service: String,
    dir: PathBuf,
}

impl Credentials {
    pub fn system(dir: &Path) -> Self {
        let service = std::env::var("BRAMLEY_KEYRING_SERVICE")
            .ok()
            .filter(|s| !s.is_empty());
        let service = service.as_deref().unwrap_or(DEFAULT_SERVICE);
        Credentials::new(native_store(), service, dir)
    }

    pub fn new(store: Option<Arc<CredentialStore>>, service: &str, dir: &Path) -> Self {
        Credentials {
            store,
            service: service.to_owned(),
            dir: dir.to_owned(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn keyring<T: Send + 'static>(
        &self,
        wait: Duration,
        op: impl FnOnce(&Entry) -> keyring_core::Result<T> + Send + 'static,
    ) -> Option<keyring_core::Result<T>> {
        let (store, service) = (self.store.clone()?, self.service.clone());
        bounded(wait, move || {
            store.build(&service, USER_TOKEN, None).and_then(|e| op(&e))
        })
    }

    pub fn load(&self) -> Option<String> {
        match self.keyring(QUICK, Entry::get_password) {
            Some(Ok(token)) => Some(token.trim().to_owned()).filter(|t| !t.is_empty()),
            Some(Err(Error::NoEntry)) => {
                let token = crate::read_token(&self.dir, USER_TOKEN).ok()?;
                let copy = token.clone();
                if !matches!(
                    self.keyring(QUICK, move |e| e.set_password(&copy)),
                    Some(Ok(()))
                ) {
                    self.warn();
                }
                Some(token)
            }
            _ => {
                self.warn();
                crate::read_token(&self.dir, USER_TOKEN).ok()
            }
        }
    }

    pub fn save(&self, token: &str) -> bool {
        let copy = token.to_owned();
        if matches!(
            self.keyring(PATIENT, move |e| e.set_password(&copy)),
            Some(Ok(()))
        ) {
            return true;
        }
        self.warn();
        write_private(&self.dir.join(USER_TOKEN), token).is_ok()
    }

    pub fn clear(&self) -> bool {
        let keyring = self.store.is_none()
            || matches!(
                self.keyring(QUICK, Entry::delete_credential),
                Some(Ok(()) | Err(Error::NoEntry))
            );
        let file = match std::fs::remove_file(self.dir.join(USER_TOKEN)) {
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
            Ok(()) => true,
        };
        keyring && file
    }

    fn warn(&self) {
        if !WARNED.swap(true, Ordering::SeqCst) {
            eprintln!(
                "bramley: WARNING: the system keyring is unavailable, so the Apple Music sign-in is kept in {} (owner-only)",
                self.dir.join(USER_TOKEN).display()
            );
        }
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

fn write_private(path: &Path, token: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    writeln!(file, "{token}")
}

fn native_store() -> Option<Arc<CredentialStore>> {
    static STORE: OnceLock<Option<Arc<CredentialStore>>> = OnceLock::new();
    let open = || {
        #[cfg(target_os = "linux")]
        let store = zbus_secret_service_keyring_store::Store::new();
        #[cfg(target_os = "macos")]
        let store = apple_native_keyring_store::keychain::Store::new();
        #[cfg(windows)]
        let store = windows_native_keyring_store::Store::new();
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        return store.ok().map(|s| s as Arc<CredentialStore>);
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        None
    };
    STORE.get_or_init(|| bounded(QUICK, open).flatten()).clone()
}
