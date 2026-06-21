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
