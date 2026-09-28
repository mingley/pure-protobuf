//! External consumer proof for the public transport byte-budget API.

use pbrs_grpc::{ByteBudgetTracker, BytePermit, Status};

#[test]
fn public_byte_budget_reclaims_permits() -> Result<(), Status> {
    let tracker = ByteBudgetTracker::with_limit(32);
    let permit: BytePermit = tracker.try_acquire(24)?;
    assert_eq!(permit.bytes(), 24);
    assert_eq!(tracker.allocated(), 24);
    assert_eq!(tracker.available(), Some(8));
    drop(permit);
    assert!(tracker.is_quiescent());
    Ok(())
}
