//! Collection aliases that work under both `std` and `no_std + alloc`.
//!
//! axiom is zero-dependency (the only optional dep is `serde`). To keep that
//! property while supporting `no_std + alloc`, we map the collection types to
//! the standard library under `std` and to `alloc`'s tree-based collections
//! under `no_std`:
//!
//! | Alias | `std` | `no_std + alloc` |
//! |-------|-------|------------------|
//! | `HashMap` | `std::collections::HashMap` | `alloc::collections::BTreeMap` |
//! | `HashSet` | `std::collections::HashSet` | `alloc::collections::BTreeSet` |
//! | `VecDeque` | `std::collections::VecDeque` | `alloc::collections::VecDeque` |
//!
//! Keys used across axiom (`&str`, `String`, `&'static str`, `(&'static str, TypeId)`)
//! are all `Ord + Eq`, so `BTreeMap`/`BTreeSet` are drop-in. The trade-off is
//! O(log n) vs O(1) lookups under `no_std`; for topology-sized graphs this is
//! immaterial, and embedders who need O(1) can vendor `hashbrown` themselves.
//!
//! Callers that want to construct a map handed to an axiom API should import
//! the alias from here (`use axiom::compat::HashMap`) so the type matches under
//! either feature configuration.

#[cfg(feature = "std")]
pub use std::collections::{HashMap, HashSet, VecDeque};

#[cfg(not(feature = "std"))]
pub use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet, VecDeque};

// ── no_std prelude shim ───────────────────────────────────────────────────────
//
// Under `#![no_std]`, `Box`/`Vec`/`String` and the `vec!` macro are NOT in the
// default prelude. This module re-exports them from `alloc` so that other
// modules can pull them in with a single glob import:
//
//   #[cfg(not(feature = "std"))]
//   use crate::compat::prelude::*;
//
// Under `std` these types are already in the prelude, so the module (and the
// gated import) is a no-op.
#[cfg(not(feature = "std"))]
pub mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
    pub use alloc::vec;
}
