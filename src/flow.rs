/// **Maturity: stable** (the stable core, main subject of the current refactor).
///
/// FlowKind — semantic dimension of a port, orthogonal to direction.
///
/// In the physical layer, all flows are the same: data moving between memory
/// addresses. The distinction between "data", "control", and "observation" is
/// a semantic label on the port — it tells the reader what kind of information
/// crosses this boundary, not how it crosses it.
///
/// # Semantics
///
/// | Kind | Meaning | Example |
/// |------|---------|---------|
/// | `Data` | Information the module processes. Changes state content. | BarEvent, Trade, Signal |
/// | `Control` | Instruction that changes module behavior. | Config change, mode switch, stop signal |
/// | `Observe` | State snapshot for external consumption. | Metrics, health, logs |
///
/// # Physical note
/// The same data stream may be interpreted as Control by the receiver and
/// as Data by an observer. The label is a contract, not a property of bits.

/// The semantic kind of data flowing through a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum FlowKind {
    /// Data flow: information processed by the module, changing state content.
    Data,
    /// Control flow: instruction that changes module behavior or configuration.
    Control,
    /// Observation flow: state snapshot for external consumption, does not
    /// change the module's behavior.
    Observe,
}

impl FlowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlowKind::Data => "data",
            FlowKind::Control => "control",
            FlowKind::Observe => "observe",
        }
    }

    /// The **materialization preference** derived from this
    /// semantic annotation.
    ///
    /// `FlowKind` is a semantic label, not a physical property (§2.3 unified
    /// value-flow principle). From the receiver-side semantics it implies an *optional*
    /// physical carrier preference:
    ///
    /// - [`FlowKind::Observe`] — "best-effort, must not back-pressure the
    ///   source" → prefer non-blocking / dropping carriers.
    /// - [`FlowKind::Control`] — "droppable, latest wins" → prefer
    ///   droppable / latest-wins carriers.
    /// - [`FlowKind::Data`] — no constraint (this is also the un-annotated
    ///   case; the matrix only applies to explicitly annotated edges).
    ///
    /// The compatibility matrix lives in the validation layer
    /// ([`crate::deploy::carrier_compatible`]) and classifies each
    /// `(FlowKind, LinkKind)` pair as [`CarrierCompatibility::Recommended`],
    /// [`CarrierCompatibility::Permitted`], or
    /// [`CarrierCompatibility::Violates`].
    pub fn carrier_compatibility(&self) -> CarrierCompatibility {
        match self {
            FlowKind::Data => CarrierCompatibility::Any,
            FlowKind::Observe => CarrierCompatibility::NonBlocking,
            FlowKind::Control => CarrierCompatibility::LatestWins,
        }
    }
}

/// The carrier category a [`FlowKind`] semantic annotation prefers (or
/// tolerates) — the abstract side of the materialization compatibility matrix.
///
/// The physical side is a concrete [`crate::link::LinkKind`]; the matrix
/// ([`crate::deploy::carrier_compatible`]) maps each kind onto one of these
/// categories and compares against the flow's requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierCompatibility {
    /// No carrier constraint (Data, or the un-annotated default).
    Any,
    /// Prefer carriers that never block / back-pressure the producer.
    NonBlocking,
    /// Prefer carriers that drop on overflow or keep only the latest value.
    LatestWins,
}

/// The three-tier result of the `(FlowKind, LinkKind)` compatibility matrix.
///
/// See [`crate::deploy::carrier_compatible`] for the full matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierCompatResult {
    /// The carrier fully satisfies the semantic preference.
    Recommended,
    /// The carrier does not violate the preference but is suboptimal
    /// (e.g. a lossless carrier for a droppable Control flow). Accepted
    /// silently — no validation finding.
    Permitted,
    /// The carrier violates the semantic contract (e.g. a blocking carrier
    /// for an Observe flow). Rejected by `validate_deep`.
    Violates,
}

impl Default for FlowKind {
    fn default() -> Self {
        FlowKind::Data
    }
}

impl core::fmt::Display for FlowKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
