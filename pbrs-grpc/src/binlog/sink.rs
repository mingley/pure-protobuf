//! Pluggable binary-log sinks (gRFC A16 "Pluggability").
//!
//! A sink receives every [`LogRecord`] the loggers emit. Sinks must be
//! cheap and non-blocking: entries are emitted inline on call paths.
#![allow(
    clippy::disallowed_types,
    reason = "short std Mutexes held only across a Vec push or File write; never across await"
)]

use super::entry::GrpcLogEntry;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// One emitted log entry: the structured value plus its wire encoding.
#[derive(Clone, Debug)]
pub struct LogRecord {
    /// The structured entry.
    pub entry: GrpcLogEntry,
    /// [`GrpcLogEntry::encode`] of [`Self::entry`], computed once at emit.
    pub bytes: Vec<u8>,
}

/// Receives binary-log records.
pub trait Sink: Send + Sync + std::fmt::Debug + 'static {
    /// Handle one record. Called inline on RPC paths; keep it fast and
    /// infallible — a sink must never fail the call it observes.
    fn emit(&self, record: &LogRecord);
}

/// Keeps every record in memory. Tests and short debug sessions use this.
#[derive(Debug, Default)]
pub struct VecSink {
    records: Mutex<Vec<LogRecord>>,
}

impl VecSink {
    /// An empty in-memory sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the records emitted so far.
    #[must_use]
    pub fn records(&self) -> Vec<LogRecord> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Number of records emitted so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether no record has been emitted yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Sink for VecSink {
    fn emit(&self, record: &LogRecord) {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(record.clone());
    }
}

/// Appends varint-length-delimited `GrpcLogEntry` records to a file.
///
/// Each record is a varint byte length followed by that many wire bytes,
/// so readers split the file without parsing. Appends never truncate: the
/// file grows until rotated externally.
#[derive(Debug)]
pub struct FileSink {
    file: Mutex<File>,
}

/// Opening or writing the log file failed.
#[derive(Debug)]
pub struct FileSinkError {
    message: String,
}

impl FileSinkError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FileSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "binary log file sink: {}", self.message)
    }
}

impl std::error::Error for FileSinkError {}

impl FileSink {
    /// Open (creating and appending to) the log file at `path`.
    pub fn open(path: &Path) -> Result<Self, FileSinkError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| FileSinkError::new(format!("open failed: {e}")))?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl Sink for FileSink {
    fn emit(&self, record: &LogRecord) {
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        let mut len = Vec::with_capacity(10);
        let mut value = u64::try_from(record.bytes.len()).unwrap_or(u64::MAX);
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            len.push(byte);
            if value == 0 {
                break;
            }
        }
        // A sink must never fail the call it observes.
        file.write_all(&len).ok();
        file.write_all(&record.bytes).ok();
    }
}
