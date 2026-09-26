//! Policy providers: static JSON or a watched file (gRFC A43 API).
//!
//! [`StaticDataProvider`] parses one JSON string. [`FileWatcherProvider`]
//! loads a file and re-reads it every `interval`: the first load must
//! succeed or construction fails, while later failures keep the last valid
//! policy and surface through [`FileWatcherProvider::last_error`].

#![allow(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "watcher Mutexes guard an Arc swap and live on a dedicated blocking thread, never across await"
)]

use super::policy::{Policy, PolicyError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Policy from a static JSON string. Always serves the same engines.
#[derive(Clone, Debug)]
pub struct StaticDataProvider {
    policy: Arc<Policy>,
}

impl StaticDataProvider {
    /// Parse `json` into a provider. Any schema violation fails here.
    pub fn new(json: &str) -> Result<Self, PolicyError> {
        Ok(Self {
            policy: Arc::new(Policy::from_json(json)?),
        })
    }

    /// The policy.
    #[must_use]
    pub fn policy(&self) -> &Arc<Policy> {
        &self.policy
    }
}

/// Policy from a file, reloaded every `interval`.
///
/// A background thread re-reads the file; unchanged bytes skip parsing.
/// A failed reload keeps serving the last valid policy and records the
/// error in [`Self::last_error`]. Dropping the last clone stops polling.
#[derive(Clone, Debug)]
pub struct FileWatcherProvider {
    state: Arc<WatchState>,
}

#[derive(Debug)]
struct WatchState {
    path: PathBuf,
    policy: Mutex<Arc<Policy>>,
    loaded_bytes: Mutex<Vec<u8>>,
    last_error: Mutex<Option<String>>,
    shutdown: AtomicBool,
}

impl Drop for WatchState {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

impl FileWatcherProvider {
    /// Load `path` and poll it every `interval`. The first load must
    /// succeed; I/O or parse errors fail construction.
    pub fn new(path: impl AsRef<Path>, interval: Duration) -> Result<Self, PolicyError> {
        let path = path.as_ref().to_owned();
        let bytes = std::fs::read(&path).map_err(|e| PolicyError::io(path.clone(), e))?;
        let policy = Arc::new(Policy::from_json(core::str::from_utf8(&bytes).map_err(
            |_| PolicyError::message(format!("{} is not UTF-8", path.display())),
        )?)?);
        let state = Arc::new(WatchState {
            path,
            policy: Mutex::new(policy),
            loaded_bytes: Mutex::new(bytes),
            last_error: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        let worker = Arc::clone(&state);
        let interval = interval.max(Duration::from_millis(1));
        core::mem::drop(
            std::thread::Builder::new()
                .name("pbrs-authz-watch".to_owned())
                .spawn(move || {
                    while !worker.shutdown.load(Ordering::Acquire) {
                        std::thread::sleep(interval);
                        if worker.shutdown.load(Ordering::Acquire) {
                            break;
                        }
                        worker.refresh_once();
                    }
                }),
        );
        Ok(Self { state })
    }

    /// The latest valid policy.
    #[must_use]
    pub fn policy(&self) -> Arc<Policy> {
        self.state.policy.lock().map_or_else(
            |poisoned| Arc::clone(&poisoned.into_inner()),
            |guard| Arc::clone(&guard),
        )
    }

    /// The last reload failure, if the latest check failed.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.state
            .last_error
            .lock()
            .map_or(None, |guard| guard.clone())
    }

    /// Run one synchronous reload check now. Returns whether the policy
    /// changed. A failed check keeps the old policy and records the error.
    pub fn refresh(&self) -> bool {
        self.state.refresh_once()
    }
}

impl WatchState {
    /// Returns true when the policy changed.
    fn refresh_once(&self) -> bool {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.set_error(format!("cannot read {}: {e}", self.path.display()));
                return false;
            }
        };
        if self.loaded_bytes.lock().is_ok_and(|guard| *guard == bytes) {
            return false;
        }
        let text = match core::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                self.set_error(format!("{} is not UTF-8", self.path.display()));
                return false;
            }
        };
        match Policy::from_json(text) {
            Ok(policy) => {
                if let Ok(mut guard) = self.policy.lock() {
                    *guard = Arc::new(policy);
                }
                if let Ok(mut guard) = self.loaded_bytes.lock() {
                    *guard = bytes;
                }
                self.clear_error();
                true
            }
            Err(e) => {
                self.set_error(e.to_string());
                false
            }
        }
    }

    fn set_error(&self, message: String) {
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = Some(message);
        }
    }

    fn clear_error(&self) {
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = None;
        }
    }
}

/// Either provider kind, for servers and interceptors.
#[derive(Clone, Debug)]
pub enum Provider {
    /// [`StaticDataProvider`].
    Static(StaticDataProvider),
    /// [`FileWatcherProvider`].
    Watching(FileWatcherProvider),
}

impl Provider {
    /// The currently valid policy.
    #[must_use]
    pub fn policy(&self) -> Arc<Policy> {
        match self {
            Self::Static(p) => Arc::clone(p.policy()),
            Self::Watching(p) => p.policy(),
        }
    }
}

impl From<StaticDataProvider> for Provider {
    fn from(provider: StaticDataProvider) -> Self {
        Self::Static(provider)
    }
}

impl From<FileWatcherProvider> for Provider {
    fn from(provider: FileWatcherProvider) -> Self {
        Self::Watching(provider)
    }
}
