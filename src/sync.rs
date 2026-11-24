//! Locks around the process-wide plugin state.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks plugin state, recovering from poisoning so a panic in one handler
/// does not wedge every later event.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
