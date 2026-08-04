/// EntityRoot: the trivial root of a system.
///
/// `EntityRoot = (S = (), name = "root")`
///
/// In category theory terms, this is the initial object of the system:
/// it has no inputs, no outputs, no process — it simply *exists*.
/// Every system can reference EntityRoot as the origin of its topology.
///
/// V8's RootModule corresponded to (Unit, ∅, Γ_sys, δ_root, ρ_root) —
/// a Machine with no state and no inputs but with a process that
/// periodically published status. EntityRoot is the decoupled form:
/// pure existence without computation. A separate "RootMonitor" Machine
/// (built on EntityRoot) handles the periodic status publishing.
use crate::entity::Entity;

pub struct EntityRoot;

impl Entity for EntityRoot {
    type State = ();

    fn name() -> &'static str { "root" }
}
