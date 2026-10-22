//! Use the portable standard mutex where Linux futexes are unavailable.

pub(super) use std::sync::Mutex;
