//! **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
//!
//! Controlled shared data — a compromise between encapsulation and composition.
//!
//! `Machine` **encapsulates** state by default (locality and verifiability:
//! state is only mutated by its own `process`), but cross-machine data sharing
//! is limited. This module provides [`SharedResource`] — a global singleton
//! that multiple computation units can declare reads/writes on — combining the
//! locality of encapsulation with data-driven compositionality (axiom's
//! controlled form of the shared-data primitive (`Resource` class)).
//!
//! # Relationship to `Machine` encapsulation
//!
//! - Default: machine state has an owner (`Machine::State` is only modified by
//!   its own `process`).
//! - Data that needs cross-machine sharing: carried **explicitly** by
//!   `SharedResource` — sharing is declarative (construct a handle and hand it
//!   to consumers), not an implicit global.
//! - Reads/writes go through `RwLock`: multiple readers can proceed in
//!   parallel, writers are mutually exclusive (corresponding to scheduling
//!   verifiability D8 — multiple writers need explicit serialization;
//!   `SharedResource::write()` is exactly the mutual-exclusion point).
//!
//! # Zero-cost note
//!
//! `SharedResource` is a **physical-layer primitive** (lock-protected shared
//! memory); it does not replace abstraction-layer port/topology validation. It
//! only introduces lock overhead when "cross-machine sharing is truly needed"
//! — machines that do not need sharing keep zero-cost encapsulation.

#[cfg(feature = "std")]
use alloc::sync::Arc;
#[cfg(feature = "std")]
use std::sync::RwLock;

/// Controlled shared data: a global singleton shared by multiple computation
/// units.
///
/// - [`read`](Self::read): shared read (multiple readers may run in parallel);
/// - [`write`](Self::write): exclusive write (writers are mutually exclusive);
/// - [`clone_handle`](Self::clone_handle): duplicate the shared handle (same
///   underlying data).
///
/// Provided only under `std` (`RwLock`); the `no_std` configuration does not
/// include this primitive.
#[cfg(feature = "std")]
pub struct SharedResource<T> {
    inner: Arc<RwLock<T>>,
}

#[cfg(feature = "std")]
impl<T> SharedResource<T> {
    /// Construct a shared resource (initial value).
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(value)),
        }
    }

    /// Shared read — returns a read guard (multiple readers may hold it in
    /// parallel).
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, T> {
        self.inner.read().expect("shared resource poisoned")
    }

    /// Exclusive write — returns a write guard (writers mutually exclusive;
    /// corresponds to D8's serialization of multiple writers).
    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
        self.inner.write().expect("shared resource poisoned")
    }

    /// Duplicate the shared handle (`Arc` clone — all handles point to the
    /// same data).
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(feature = "std")]
impl<T> Clone for SharedResource<T> {
    fn clone(&self) -> Self {
        self.clone_handle()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Unit tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn shared_resource_read_write() {
        let shared = SharedResource::new(0i32);
        {
            let mut w = shared.write();
            *w += 10;
        }
        assert_eq!(*shared.read(), 10);
    }

    #[test]
    fn shared_resource_multi_handle() {
        // Multiple handles share the same data (the controlled form of the
        // shared-data primitive (`Resource` class)).
        let shared = SharedResource::new(vec![1i32, 2, 3]);
        let handle_a = shared.clone_handle();
        let handle_b = shared.clone_handle();

        {
            let mut w = handle_a.write();
            w.push(4);
        }
        assert_eq!(handle_b.read().len(), 4, "all handles observe the same data");
    }
}
