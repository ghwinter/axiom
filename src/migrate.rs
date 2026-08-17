/// **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
///
/// SchemaMigrate — version migration for port payload schemas.
///
/// # Problem this solves
///
/// When a port's schema version changes (e.g., a struct field is added or
/// renamed), `LinkCompat::can_link_to()` returns `Migrate { from_ver, to_ver }`.
/// The runtime then needs a migrator to convert values from the old version
/// to the new version so that old and new machines can still communicate.
///
/// This module provides:
/// - [`SchemaMigrate`] trait — for strongly-typed migration implementations.
/// - [`MigrateFn`] — a type-erased migration function for runtime dispatch.
/// - [`MigrateRegistry`] — a registry of migrators, keyed by (port_name, TypeId).
///
/// # Usage
///
/// ```ignore
/// use axiom::migrate::{SchemaMigrate, MigrateRegistry};
/// use std::sync::Arc;
///
/// // V1 → V2 migration: add a default field
/// struct MyMigrator;
/// impl SchemaMigrate for MyMigrator {
///     type Value = MyPayload;
///     fn migrate(&self, value: MyPayload, from_ver: u32, to_ver: u32) -> Option<MyPayload> {
///         match (from_ver, to_ver) {
///             (0, 1) => Some(MyPayload { ..value, new_field: 0 }),
///             _ => None,
///         }
///     }
/// }
///
/// let mut registry = MigrateRegistry::new();
/// registry.register("trade_out", std::any::TypeId::of::<MyPayload>(), Arc::new(MyMigrator));
/// ```

use core::any::Any;
use alloc::boxed::Box;
use alloc::sync::Arc;

// ── SchemaMigrate trait ───────────────────────────────────────────────────────

/// Strongly-typed migration from one schema version to another.
///
/// Implement this for a type that knows how to convert its `Value` between
/// schema versions. The runtime wraps implementations into type-erased
/// `MigrateFn`s via [`MigrateRegistry::register_typed`].
pub trait SchemaMigrate: Send + Sync + 'static {
    /// The payload type being migrated.
    type Value: Any + Send + 'static;

    /// Migrate `value` from `from_ver` to `to_ver`.
    ///
    /// Implementations should handle each consecutive version step.
    /// For example, to migrate from v0 to v2, apply v0→v1 then v1→v2.
    /// Returns `None` if the migration path is not supported.
    fn migrate(&self, value: Self::Value, from_ver: u32, to_ver: u32) -> Option<Self::Value>;
}

// ── Type-erased migration function ───────────────────────────────────────────

/// A type-erased migration function.
///
/// Takes a `Box<dyn Any + Send>` payload, the source version, and the target
/// version. Returns the migrated payload, or `None` if migration fails.
///
/// This is what the runtime stores and dispatches — it doesn't know the
/// concrete payload type at compile time.
pub type MigrateFn =
    Arc<dyn Fn(Box<dyn Any + Send>, u32, u32) -> Option<Box<dyn Any + Send>> + Send + Sync>;

// ── MigrateRegistry ──────────────────────────────────────────────────────────
//
// `MigrateRegistry` uses `RwLock` for concurrent registration/lookup, which
// requires `std` — unavailable under `no_std + alloc` (see lib.rs: RwLock-based
// containers are gated to std). The `SchemaMigrate` trait and `MigrateFn` type
// only need `Arc`/`Box` (alloc) and work under no_std as well.

/// A registry of schema migrators, keyed by (port_name, TypeId).
///
/// The runtime consults this registry when `LinkCompat::can_link_to()`
/// returns `Migrate { from_ver, to_ver }` — it looks up the migrator
/// for the port's (name, type_id) and applies it to each value that
/// crosses the link.
///
/// # Thread safety
///
/// `MigrateRegistry` uses `RwLock` internally: reads (migration lookups)
/// are lock-free concurrent, writes (registrations) are exclusive.
/// In practice, registrations happen at startup and migrations happen
/// during `process()`, so contention is minimal.
#[cfg(feature = "std")]
pub struct MigrateRegistry {
    migrators: RwLock<crate::compat::HashMap<(&'static str, TypeId), MigrateFn>>,
}

// Note: We avoid importing std::sync::RwLock via `use` to keep the module
// self-contained. Re-exported here for the field type.
#[cfg(feature = "std")]
use std::sync::RwLock;

#[cfg(feature = "std")]
use crate::compat::HashMap;

#[cfg(feature = "std")]
use core::any::TypeId;

#[cfg(feature = "std")]
impl MigrateRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            migrators: RwLock::new(HashMap::new()),
        }
    }

    /// Register a type-erased migrator for a (port_name, type_id) pair.
    ///
    /// If a migrator is already registered for this key, it is replaced.
    pub fn register(
        &self,
        port_name: &'static str,
        type_id: TypeId,
        migrator: MigrateFn,
    ) {
        self.migrators.write().expect("MigrateRegistry poisoned")
            .insert((port_name, type_id), migrator);
    }

    /// Register a strongly-typed `SchemaMigrate` implementation.
    ///
    /// This wraps the migrator into a type-erased `MigrateFn` that
    /// downcasts the input, calls `migrate()`, and re-boxes the output.
    pub fn register_typed<M: SchemaMigrate>(&self, port_name: &'static str, migrator: M) {
        let arc: Arc<M> = Arc::new(migrator);
        let type_id = TypeId::of::<M::Value>();
        let migrate_fn: MigrateFn = Arc::new(move |value: Box<dyn Any + Send>, from_ver, to_ver| {
            let typed = value.downcast::<M::Value>().ok()?;
            let result = arc.migrate(*typed, from_ver, to_ver)?;
            Some(Box::new(result) as Box<dyn Any + Send>)
        });
        self.register(port_name, type_id, migrate_fn);
    }

    /// Look up a migrator and apply it to a value.
    ///
    /// Returns `None` if no migrator is registered for the (port_name, type_id)
    /// pair, or if the migrator returns `None` (migration path not supported).
    pub fn migrate(
        &self,
        port_name: &str,
        type_id: TypeId,
        value: Box<dyn Any + Send>,
        from_ver: u32,
        to_ver: u32,
    ) -> Option<Box<dyn Any + Send>> {
        let guard = self.migrators.read().expect("MigrateRegistry poisoned");
        // Note: we need to find by (port_name, type_id), but port_name is
        // &str, not &'static str. We compare by string content.
        let key = guard.keys().find(|(name, tid)| *name == port_name && *tid == type_id)?;
        let migrator = guard.get(key)?;
        migrator(value, from_ver, to_ver)
    }

    /// Check whether a migrator is registered for the given (port_name, type_id).
    pub fn has_migrator(&self, port_name: &str, type_id: TypeId) -> bool {
        let guard = self.migrators.read().expect("MigrateRegistry poisoned");
        guard.keys().any(|(name, tid)| *name == port_name && *tid == type_id)
    }

    /// Number of registered migrators.
    pub fn len(&self) -> usize {
        self.migrators.read().expect("MigrateRegistry poisoned").len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(feature = "std")]
impl Default for MigrateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl core::fmt::Debug for MigrateRegistry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let guard = self.migrators.read().expect("MigrateRegistry poisoned");
        f.debug_struct("MigrateRegistry")
            .field("migrator_count", &guard.len())
            .finish()
    }
}
