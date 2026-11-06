//! Policy providers: static JSON or a watched file (gRFC A43 API).
//!
//! [`StaticDataProvider`] parses one JSON string. [`FileWatcherProvider`]
//! loads a file and re-reads it every `interval`: the first load must
//! succeed or construction fails, while later failures keep the last valid
//! policy and surface through [`FileWatcherProvider::last_error`].

#![allow(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "policy snapshots use a short Arc mutex; reload I/O has a separate control-plane mutex, never across await"
)]

use super::policy::{Policy, PolicyError};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
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
/// error in [`Self::last_error`]. Dropping the last clone wakes an idle worker
/// immediately; an in-progress reload can finish without blocking the drop.
#[derive(Clone, Debug)]
pub struct FileWatcherProvider {
    state: Arc<WatchState>,
}

#[derive(Debug)]
struct WatchState {
    // Only the state owns a sender. Drop it before policy cleanup so an idle
    // worker wakes without joining or waiting for the polling interval.
    _worker_lifetime: Sender<()>,
    path: PathBuf,
    policy: Mutex<Arc<Policy>>,
    reload: Mutex<()>,
    loaded_bytes: Mutex<Vec<u8>>,
    last_error: Mutex<Option<String>>,
}

impl FileWatcherProvider {
    /// Load `path` and poll it every `interval`. The first load must
    /// succeed; I/O or parse errors fail construction.
    pub fn new(path: impl AsRef<Path>, interval: Duration) -> Result<Self, PolicyError> {
        let (provider, worker) =
            Self::new_with_spawner(path, interval, |state, stop, interval| {
                std::thread::Builder::new()
                    .name("pbrs-authz-watch".to_owned())
                    .spawn(move || run_watcher(state, stop, interval))
            })?;
        drop(worker);
        Ok(provider)
    }

    fn new_with_spawner(
        path: impl AsRef<Path>,
        interval: Duration,
        spawn: impl FnOnce(Weak<WatchState>, Receiver<()>, Duration) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<(Self, JoinHandle<()>), PolicyError> {
        let (state, stop) = WatchState::load(path.as_ref().to_owned())?;
        let worker = spawn(
            Arc::downgrade(&state),
            stop,
            interval.max(Duration::from_millis(1)),
        )
        .map_err(|error| {
            PolicyError::message(format!(
                "cannot start policy watcher for {}: {error}",
                state.path.display()
            ))
        })?;
        Ok((Self { state }, worker))
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Run one synchronous reload check now. Returns whether this check
    /// changed the policy. Another updater may have already applied the bytes.
    /// A failed check keeps the old policy and records the error.
    pub fn refresh(&self) -> bool {
        self.state.refresh_once()
    }
}

fn run_watcher(state: Weak<WatchState>, stop: Receiver<()>, interval: Duration) {
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(interval) {
        let Some(state) = state.upgrade() else {
            break;
        };
        state.refresh_once();
        // The upgraded Arc drops here, before the next blocking wait.
    }
}

impl WatchState {
    fn load(path: PathBuf) -> Result<(Arc<Self>, Receiver<()>), PolicyError> {
        let bytes = std::fs::read(&path).map_err(|e| PolicyError::io(path.clone(), e))?;
        let policy = Arc::new(Policy::from_json(core::str::from_utf8(&bytes).map_err(
            |_| PolicyError::message(format!("{} is not UTF-8", path.display())),
        )?)?);
        let (worker_lifetime, stop) = mpsc::channel();
        let state = Arc::new(Self {
            path,
            policy: Mutex::new(policy),
            reload: Mutex::new(()),
            loaded_bytes: Mutex::new(bytes),
            last_error: Mutex::new(None),
            _worker_lifetime: worker_lifetime,
        });
        Ok((state, stop))
    }

    /// Returns true when the policy changed.
    fn refresh_once(&self) -> bool {
        self.refresh_with(|path| std::fs::read(path))
    }

    fn refresh_with(&self, read: impl FnOnce(&Path) -> std::io::Result<Vec<u8>>) -> bool {
        // Serialize file read, parsing and publication. Request snapshots only
        // use `policy`, so file I/O never holds their mutex.
        let reload = self
            .reload
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = match read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.set_error(format!("cannot read {}: {e}", self.path.display()));
                return false;
            }
        };
        if *self
            .loaded_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            == bytes
        {
            self.clear_error();
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
                let policy = Arc::new(policy);
                let previous = {
                    let mut guard = self
                        .policy
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    core::mem::replace(&mut *guard, policy)
                };
                *self
                    .loaded_bytes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = bytes;
                self.clear_error();
                drop(reload);
                drop(previous);
                true
            }
            Err(e) => {
                self.set_error(e.to_string());
                false
            }
        }
    }

    fn set_error(&self, message: String) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message);
    }

    fn clear_error(&self) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "private watcher controls use bounded channel waits and local temporary files"
)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OPEN: &str = r#"{"name":"open","allow_rules":[{"name":"all"}]}"#;
    const CLOSED: &str = r#"{"name":"closed","allow_rules":[]}"#;
    const WAIT: Duration = Duration::from_secs(5);
    static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

    struct PolicyFile(PathBuf);

    impl PolicyFile {
        fn new() -> Self {
            loop {
                let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "pbrs-authz-provider-{}-{id}.json",
                    std::process::id()
                ));
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(mut file) => {
                        file.write_all(OPEN.as_bytes()).expect("initial policy");
                        return Self(path);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("create policy fixture: {error}"),
                }
            }
        }

        fn write(&self, bytes: impl AsRef<[u8]>) {
            std::fs::write(&self.0, bytes).expect("write policy fixture");
        }

        fn without_worker(&self) -> FileWatcherProvider {
            let (state, _stop) = WatchState::load(self.0.clone()).expect("initial load");
            FileWatcherProvider { state }
        }
    }

    impl Drop for PolicyFile {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    #[test]
    fn manual_refresh_reports_changed_and_unchanged_without_poller() {
        let file = PolicyFile::new();
        let provider = file.without_worker();
        let original = provider.policy();
        assert!(!provider.refresh());
        assert!(Arc::ptr_eq(&provider.policy(), &original));

        file.write(CLOSED);
        assert!(provider.refresh());
        let closed = provider.policy();
        assert_eq!(closed.name(), "closed");
        assert!(!Arc::ptr_eq(&closed, &original));
        assert!(!provider.refresh());
        assert!(Arc::ptr_eq(&closed, &provider.policy()));
        assert!(provider.last_error().is_none());
    }

    #[test]
    fn identical_valid_bytes_clear_previous_read_parse_and_utf8_errors() {
        let file = PolicyFile::new();
        let provider = file.without_worker();
        let original = provider.policy();
        assert!(
            !provider
                .state
                .refresh_with(|_| Err(std::io::Error::other("reader failed")))
        );
        assert!(
            provider
                .last_error()
                .expect("I/O error")
                .contains("reader failed")
        );
        assert!(!provider.refresh());
        assert!(provider.last_error().is_none());

        for broken in [b"{broken".as_slice(), b"\xff".as_slice()] {
            file.write(broken);
            assert!(!provider.refresh());
            assert!(provider.last_error().is_some());
            assert!(Arc::ptr_eq(&provider.policy(), &original));
            file.write(OPEN);
            assert!(!provider.refresh(), "restored bytes are already loaded");
            assert!(provider.last_error().is_none());
            assert!(Arc::ptr_eq(&provider.policy(), &original));
        }
    }

    #[test]
    fn last_clone_wakes_long_interval_worker_and_releases_state() {
        let file = PolicyFile::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let (provider, worker) = FileWatcherProvider::new_with_spawner(
            &file.0,
            Duration::from_secs(3600),
            |state, stop, interval| {
                std::thread::Builder::new().spawn(move || {
                    started_tx.send(()).expect("worker started");
                    run_watcher(state, stop, interval);
                    finished_tx.send(()).expect("worker finished");
                })
            },
        )
        .expect("start worker");
        started_rx.recv_timeout(WAIT).expect("worker scheduled");
        let state = Arc::downgrade(&provider.state);
        let last = provider.clone();
        drop(provider);
        assert!(
            state.upgrade().is_some(),
            "the remaining clone owns the state"
        );
        assert_eq!(finished_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
        drop(last);
        assert!(
            state.upgrade().is_none(),
            "an idle worker must not retain the state"
        );
        finished_rx
            .recv_timeout(WAIT)
            .expect("drop must wake the worker before its hour-long interval");
        worker.join().expect("worker exits cleanly");
    }

    #[test]
    fn worker_spawn_failure_is_reported_and_releases_state() {
        let file = PolicyFile::new();
        let mut observed = None;
        let error = FileWatcherProvider::new_with_spawner(
            &file.0,
            Duration::ZERO,
            |state, _stop, interval| {
                assert_eq!(interval, Duration::from_millis(1));
                observed = Some(state);
                Err(std::io::Error::other("thread unavailable"))
            },
        )
        .expect_err("spawn error must fail construction");
        assert!(error.to_string().contains("cannot start policy watcher"));
        assert!(error.to_string().contains("thread unavailable"));
        assert!(observed.expect("observed weak state").upgrade().is_none());
    }

    #[test]
    fn refresh_serializes_read_and_publication_without_blocking_policy_snapshot() {
        let file = PolicyFile::new();
        let provider = file.without_worker();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_state = Arc::clone(&provider.state);
        let first = std::thread::spawn(move || {
            first_state.refresh_with(|_| {
                entered_tx.send(()).expect("first read entered");
                release_rx.recv_timeout(WAIT).expect("release first read");
                Ok(CLOSED.as_bytes().to_vec())
            })
        });
        entered_rx.recv_timeout(WAIT).expect("first reader started");
        assert!(matches!(
            provider.state.reload.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));

        let snapshot_provider = provider.clone();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let snapshot = std::thread::spawn(move || {
            snapshot_tx
                .send(snapshot_provider.policy().name().to_owned())
                .expect("policy snapshot");
        });
        assert_eq!(
            snapshot_rx
                .recv_timeout(WAIT)
                .expect("policy reads must not wait for file I/O"),
            "open"
        );
        snapshot.join().expect("snapshot completed");

        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let second_state = Arc::clone(&provider.state);
        let second = std::thread::spawn(move || {
            attempt_tx.send(()).expect("second refresh attempted");
            second_state.refresh_with(|_| {
                second_tx.send(()).expect("second read entered");
                Ok(br#"{"name":"newest","allow_rules":[]}"#.to_vec())
            })
        });
        attempt_rx
            .recv_timeout(WAIT)
            .expect("second thread scheduled");
        assert_eq!(second_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
        release_tx.send(()).expect("finish first read");
        assert!(first.join().expect("first refresh"));
        second_rx
            .recv_timeout(WAIT)
            .expect("second read follows first reload");
        assert!(second.join().expect("second refresh"));
        assert_eq!(provider.policy().name(), "newest");
        assert!(
            !provider
                .state
                .refresh_with(|_| Ok(br#"{"name":"newest","allow_rules":[]}"#.to_vec()))
        );
        assert!(provider.last_error().is_none());
    }
}
