//! Client-side health checking input for LB policies (A17, CH-05).
//!
//! Pure mapping rules: Watch response statuses become per-address
//! health signals, and terminal RPC statuses decide whether watching
//! continues. The Watch transport loop lives in the pool next to the
//! subchannel connections; policies consume [`HealthSignal`] through
//! `note_health` and skip unhealthy addresses when picking.

use crate::health::ServingStatus;
use crate::status::{Code, Status};

/// Health signal for one subchannel address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthSignal {
    /// The backend serves (or health checking is disabled for it).
    Healthy,
    /// The backend reports anything but serving, or its Watch fails
    /// retryably. The address leaves rotation until a healthy signal.
    Unhealthy,
}

/// Map a Watch response status: only `SERVING` is healthy. `UNKNOWN`,
/// `NOT_SERVING`, `SERVICE_UNKNOWN`, and any future value all gate
/// the address out.
#[must_use]
pub fn signal_for(status: ServingStatus) -> HealthSignal {
    if status == ServingStatus::Serving {
        HealthSignal::Healthy
    } else {
        HealthSignal::Unhealthy
    }
}

/// A17 absence rule: a Watch that fails `UNIMPLEMENTED` means the
/// backend has no health service. Stop watching and treat the
/// address as healthy.
#[must_use]
pub fn disables_health_check(status: &Status) -> bool {
    status.code() == Code::Unimplemented
}

#[cfg(test)]
mod tests {
    use super::{HealthSignal, disables_health_check, signal_for};
    use crate::health::ServingStatus;
    use crate::status::{Code, Status};

    #[test]
    fn only_serving_is_healthy() {
        assert_eq!(signal_for(ServingStatus::Serving), HealthSignal::Healthy);
        for status in [
            ServingStatus::Unknown,
            ServingStatus::NotServing,
            ServingStatus::ServiceUnknown,
            ServingStatus::from(99),
        ] {
            assert_eq!(
                signal_for(status),
                HealthSignal::Unhealthy,
                "{status:?} gates out"
            );
        }
    }

    #[test]
    fn unimplemented_disables_while_errors_retry() {
        assert!(disables_health_check(&Status::unimplemented("no health")));
        for code in [
            Code::Unavailable,
            Code::Unknown,
            Code::DeadlineExceeded,
            Code::Internal,
        ] {
            assert!(
                !disables_health_check(&Status::new(code, "retryable")),
                "{code:?} keeps watching"
            );
        }
    }
}
