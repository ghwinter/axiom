/// ConfigCell — a versioned, thread-safe runtime configuration container.
///
/// # Problem this solves
///
/// `config_schema()` declares *what* configuration a Machine accepts, and
/// `MachineInstance::config_overrides` carries deploy-time values. But there
/// is no runtime mechanism for a Machine to:
/// - Read the current config value during `process()`
/// - Detect that the config has been updated since the last check
/// - Receive hot-reloaded config without restarting
///
/// `ConfigCell<T>` fills this gap. It wraps a config value with a monotonically
/// increasing version number. The runtime updates the value (bumping the
/// version); the Machine polls `check()` to detect changes.
///
/// # Usage
///
/// ```ignore
/// use axiom::config::ConfigCell;
/// use axiom::prelude_all::*;
///
/// struct MyConfig {
///     threshold: f64,
///     mode: String,
/// }
///
/// struct MyState {
///     config: ConfigCell<MyConfig>,
///     config_version: u64,
/// }
///
/// impl Machine for MyMachine {
///     type State = MyState;
///     // ...
///     fn init(ctx: &MachineContext) -> Result<MyState, InitError> {
///         Ok(MyState {
///             config: ConfigCell::new(MyConfig {
///                 threshold: 0.5,
///                 mode: "default".into(),
///             }),
///             config_version: 0,
///         })
///     }
///     fn process(state: &mut MyState, ctx: &MachineContext, input: Self::Input)
///         -> ProcessOutput<Self::Output>
///     {
///         // Check for config updates
///         if let (Some(new_cfg), new_ver) = state.config.check(state.config_version) {
///             state.config_version = new_ver;
///             // apply new config...
///         }
///         // ...
///     }
/// }
/// ```
///
/// # Thread safety
///
/// `ConfigCell` uses `RwLock` for the value and `AtomicU64` for the version.
/// Multiple readers can read concurrently; only one writer at a time.
/// The version is updated *after* the value is written, with `Release` ordering,
/// so a reader that sees the new version is guaranteed to see the new value.

use std::sync::RwLock;
use core::sync::atomic::{AtomicU64, Ordering};

// ── ConfigCell ───────────────────────────────────────────────────────────────

/// A versioned, thread-safe configuration container.
///
/// Stores a config value `T` alongside a monotonically increasing version
/// number. Readers can detect changes by comparing version numbers.
pub struct ConfigCell<T> {
    value: RwLock<T>,
    version: AtomicU64,
}

impl<T: Clone + Send + Sync> ConfigCell<T> {
    /// Create a new ConfigCell with an initial value.
    /// The initial version is 0.
    pub fn new(initial: T) -> Self {
        Self {
            value: RwLock::new(initial),
            version: AtomicU64::new(0),
        }
    }

    /// Get the current config value and its version.
    ///
    /// Returns `(value, version)`. The value is cloned.
    pub fn get(&self) -> (T, u64) {
        let ver = self.version.load(Ordering::Acquire);
        let val = self.value.read().expect("ConfigCell poisoned").clone();
        (val, ver)
    }

    /// Check if the config has changed since `last_seen` version.
    ///
    /// Returns `(Some(new_value), new_version)` if the config has been
    /// updated since `last_seen`. Returns `(None, last_seen)` if unchanged.
    ///
    /// This is the primary API for Machines to poll config changes
    /// during `process()` — cheap when unchanged (no clone, no lock).
    pub fn check(&self, last_seen: u64) -> (Option<T>, u64) {
        let current = self.version.load(Ordering::Acquire);
        if current != last_seen {
            let val = self.value.read().expect("ConfigCell poisoned").clone();
            (Some(val), current)
        } else {
            (None, last_seen)
        }
    }

    /// Update the config value. Bumps the version number by 1.
    ///
    /// Called by the runtime or deployer to push a new config.
    /// The version is incremented *after* the value is written,
    /// ensuring that any reader observing the new version sees the new value.
    pub fn update(&self, new: T) {
        *self.value.write().expect("ConfigCell poisoned") = new;
        self.version.fetch_add(1, Ordering::Release);
    }

    /// Current version number (without reading the value).
    /// Useful for quick change-detection without cloning.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
}

impl<T: Clone + Send + Sync + core::fmt::Debug> core::fmt::Debug for ConfigCell<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let ver = self.version.load(Ordering::Relaxed);
        let val = self.value.read().expect("ConfigCell poisoned");
        f.debug_struct("ConfigCell")
            .field("version", &ver)
            .field("value", &*val)
            .finish()
    }
}

impl<T: Clone + Send + Sync + Default> Default for ConfigCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// ── ConfigError ──────────────────────────────────────────────────────────────

/// Errors that can occur during config operations.
#[derive(Debug)]
pub enum ConfigError {
    /// The config key was not found.
    KeyNotFound(&'static str),
    /// The config value could not be deserialized to the expected type.
    TypeMismatch { key: &'static str, expected: &'static str },
    /// The config value was invalid.
    InvalidValue { key: &'static str, reason: String },
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::KeyNotFound(k) => write!(f, "config key not found: {}", k),
            Self::TypeMismatch { key, expected } => {
                write!(f, "config type mismatch for '{}': expected {}", key, expected)
            }
            Self::InvalidValue { key, reason } => {
                write!(f, "invalid config value for '{}': {}", key, reason)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ConfigError {}
