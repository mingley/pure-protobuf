//! A3 bounded channel traces: one ring per channel, subchannel, or server.
//!
//! A trace keeps the newest `max_events` events plus the total ever
//! logged, so memory is flat under churn: `O(max_events)` short
//! strings per entity. Descriptions are truncated to
//! [`MAX_DESCRIPTION_LEN`] chars, so one verbose event (a long
//! target name, an error chain) cannot blow the budget either.
//! Events are infrequent by construction — creation, deletion,
//! connectivity transitions, interesting resolution changes — never
//! per-RPC.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::SystemTime;

/// Default cap on retained trace events per entity. Matches the
/// common cross-language default; override per trace with
/// [`Trace::with_max_events`].
pub const DEFAULT_MAX_TRACE_EVENTS: usize = 30;

/// Descriptions longer than this (in chars) are truncated with a
/// trailing ellipsis marker.
pub const MAX_DESCRIPTION_LEN: usize = 512;

/// A3 severity, mirroring the proto enum one-to-one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraceSeverity {
    /// Unknown (proto default; never logged, only read back).
    #[default]
    Unknown,
    /// Informational event.
    Info,
    /// Suspicious but non-fatal event.
    Warning,
    /// Failure event.
    Error,
}

/// A child entity referenced by a trace event (A3 `child_ref`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceChild {
    /// A channel created under (or otherwise referenced by) the traced entity.
    Channel {
        /// Referenced channel id.
        id: u64,
        /// Referenced channel name.
        name: String,
    },
    /// A subchannel created under the traced channel.
    Subchannel {
        /// Referenced subchannel id.
        id: u64,
        /// Referenced subchannel name.
        name: String,
    },
}

/// One retained trace event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    /// Short human-readable description (truncated at log time).
    pub description: String,
    /// Event severity.
    pub severity: TraceSeverity,
    /// When the event was logged.
    pub at: SystemTime,
    /// Referenced child, if the event is about one.
    pub child: Option<TraceChild>,
}

/// Immutable snapshot of a trace for the channelz service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSnapshot {
    /// When the traced entity was created.
    pub created: SystemTime,
    /// Events ever logged (grows past the retained ring).
    pub num_logged: u64,
    /// Retained events, oldest first.
    pub events: Vec<TraceEvent>,
}

#[derive(Debug)]
struct TraceState {
    events: VecDeque<TraceEvent>,
    num_logged: u64,
}

/// Bounded A3 trace ring. Cheap to share; logging takes one small
/// mutex (events are infrequent, never per-RPC).
#[derive(Debug)]
pub struct Trace {
    created: SystemTime,
    max_events: usize,
    state: Mutex<TraceState>,
}

impl Trace {
    /// New trace with the default event cap.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_events(DEFAULT_MAX_TRACE_EVENTS)
    }

    /// New trace with an explicit event cap. Zero keeps no events
    /// (the total still counts).
    #[must_use]
    pub fn with_max_events(max_events: usize) -> Self {
        Self {
            created: SystemTime::now(),
            max_events,
            state: Mutex::new(TraceState {
                events: VecDeque::new(),
                num_logged: 0,
            }),
        }
    }

    /// Log one event, evicting the oldest past the cap. The total
    /// counts every call even when nothing is retained.
    pub fn log(
        &self,
        severity: TraceSeverity,
        description: impl Into<String>,
        child: Option<TraceChild>,
    ) {
        let event = TraceEvent {
            description: truncate(description.into()),
            severity,
            at: SystemTime::now(),
            child,
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.num_logged = state.num_logged.saturating_add(1);
        if self.max_events == 0 {
            return;
        }
        state.events.push_back(event);
        while state.events.len() > self.max_events {
            state.events.pop_front();
        }
    }

    /// Log an informational event without a child ref.
    pub fn info(&self, description: impl Into<String>) {
        self.log(TraceSeverity::Info, description, None);
    }

    /// Log a warning event without a child ref.
    pub fn warning(&self, description: impl Into<String>) {
        self.log(TraceSeverity::Warning, description, None);
    }

    /// Log an error event without a child ref.
    pub fn error(&self, description: impl Into<String>) {
        self.log(TraceSeverity::Error, description, None);
    }

    /// Snapshot the trace: creation time, total logged, and retained
    /// events oldest-first.
    #[must_use]
    pub fn snapshot(&self) -> TraceSnapshot {
        let state = self.state.lock().ok();
        let (events, num_logged) = state
            .as_ref()
            .map(|state| (state.events.iter().cloned().collect(), state.num_logged))
            .unwrap_or_default();
        TraceSnapshot {
            created: self.created,
            num_logged,
            events,
        }
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}

/// Truncate to [`MAX_DESCRIPTION_LEN`] chars on a char boundary,
/// marking the cut. Short strings pass through untouched.
fn truncate(mut description: String) -> String {
    // Byte length bounds char count from above, so byte-short
    // strings pass here; anything longer gets an exact char count.
    if description.len() <= MAX_DESCRIPTION_LEN {
        return description;
    }
    if description.chars().count() <= MAX_DESCRIPTION_LEN {
        return description;
    }
    let end = description
        .char_indices()
        .take(MAX_DESCRIPTION_LEN)
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    description.truncate(end);
    description.push('…');
    description
}

#[cfg(test)]
mod tests {
    use super::{MAX_DESCRIPTION_LEN, Trace, TraceChild, TraceSeverity};

    #[test]
    fn ring_evicts_oldest_and_counts_total() {
        let trace = Trace::with_max_events(3);
        for i in 0..5 {
            trace.info(format!("event-{i}"));
        }
        let snapshot = trace.snapshot();
        assert_eq!(snapshot.num_logged, 5);
        let descriptions: Vec<&str> = snapshot
            .events
            .iter()
            .map(|event| event.description.as_str())
            .collect();
        assert_eq!(descriptions, vec!["event-2", "event-3", "event-4"]);
    }

    #[test]
    fn zero_cap_counts_without_retaining() {
        let trace = Trace::with_max_events(0);
        trace.info("dropped");
        let snapshot = trace.snapshot();
        assert_eq!(snapshot.num_logged, 1);
        assert!(snapshot.events.is_empty());
    }

    #[test]
    fn long_descriptions_truncate_on_char_boundary() {
        let trace = Trace::new();
        let long = "é".repeat(MAX_DESCRIPTION_LEN + 10);
        trace.log(TraceSeverity::Error, long, None);
        let snapshot = trace.snapshot();
        let description = &snapshot.events[0].description;
        assert!(description.ends_with('…'));
        assert_eq!(description.chars().count(), MAX_DESCRIPTION_LEN + 1);
        assert_eq!(snapshot.events[0].severity, TraceSeverity::Error);
    }

    #[test]
    fn child_refs_survive_the_snapshot() {
        let trace = Trace::new();
        trace.log(
            TraceSeverity::Info,
            "Subchannel created",
            Some(TraceChild::Subchannel {
                id: 7,
                name: "10.0.0.1:80".to_owned(),
            }),
        );
        let snapshot = trace.snapshot();
        assert_eq!(
            snapshot.events[0].child,
            Some(TraceChild::Subchannel {
                id: 7,
                name: "10.0.0.1:80".to_owned(),
            })
        );
    }
}
