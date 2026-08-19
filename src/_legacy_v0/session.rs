//! **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
//!
//! Session Types — protocol-level type safety for port communications.
//!
//! # Theoretical foundation
//!
//! Based on:
//! - Honda, Yoshida, Carbone "Multiparty Asynchronous Session Types"
//!   (POPL 2008, JACM 2016) — MPST foundation
//! - Honda 1993, Takeuchi 1994 — binary session types
//! - de Alfaro, Henzinger "Interface Automata" (FSE 2001) — interface
//!   compatibility checking
//!
//! This module implements **both** binary and multiparty session types:
//!
//! - **Binary** — [`SessionType`] + [`SessionState`] + [`is_dual()`]
//! - **Multiparty (MPST)** — [`GlobalType`] + [`project()`] + [`LocalType`]
//!
//! # Problem this solves
//!
//! `PortSchema` + `FlowKind` + `TypeId` ensure that two ports are
//! *type-compatible* (same Rust type, same flow kind). But they cannot
//! express *protocol* constraints:
//!
//! - "After sending a `Login`, the next send must be `Auth`, then `Request`."
//! - "After receiving a `Query`, the server must reply with either `Result`
//!    or `Error`."
//! - "A `Handshake` port alternates: Send Hello → Recv Welcome → Send Ready."
//! - "In a 3-party protocol: Client → Server → Database → Client"
//!
//! Session types solve this by treating the protocol itself as a type.
//! Each port carries a *session type* that describes the sequence of
//! allowed operations. The type system ensures that only protocol-conforming
//! sequences compile or pass validation.
//!
//! # Design
//!
//! ## Binary session types
//!
//! Encoded as a Rust enum that can be:
//! 1. **Statically checked** — via [`SessionProtocol`] trait (compile-time
//!    protocol adherence).
//! 2. **Dynamically checked** — via [`SessionState`] runtime automaton
//!    (transitions validated at each `process()` call).
//!
//! ## Multiparty session types (MPST)
//!
//! MPST describes protocols among **N ≥ 2** participants. The workflow is:
//!
//! 1. Define a [`GlobalType`] — the choreography of all participants.
//! 2. [`project()`] the global type onto each participant to get a [`LocalType`].
//! 3. Each participant's port carries its [`LocalType`] as the session protocol.
//! 4. The runtime verifies that all local types are **compliant** with the
//!    global type (i.e., the projection is consistent).
//!
//! # Grammar
//!
//! ## Binary session types
//!
//! ```text
//! S ::= !T.S      (send T, then continue as S)
//!     | ?T.S      (receive T, then continue as S)
//!     | S + S     (internal choice: pick one branch)
//!     | S & S     (external choice: accept one branch)
//!     | μX.S      (recursive, bind X)
//!     | X         (recursive variable reference)
//!     | end       (terminate)
//!     | skip      (no-op, continue)
//! ```
//!
//! ## Multiparty global types
//!
//! ```text
//! G ::= p → q : L.G   (message from role p to role q with label L, then G)
//!     | G + G          (choice between two global behaviors)
//!     | μX.G           (recursive)
//!     | X              (recursion variable)
//!     | end            (terminate)
//! ```
//!
//! # Usage
//!
//! ## Binary
//!
//! ```ignore
//! use axiom::session::{SessionType, SessionOp, SessionState};
//!
//! let login_protocol = SessionType::sequence(&[
//!     SessionOp::Send { type_name: "Login" },
//!     SessionOp::Recv { type_name: "Welcome" },
//!     SessionOp::Send { type_name: "Ready" },
//! ]);
//!
//! let mut state = SessionState::new(&login_protocol);
//! assert!(state.can_send("Login"));
//! state.advance_send("Login");
//! assert!(state.can_recv("Welcome"));
//! state.advance_recv("Welcome");
//! assert!(state.can_send("Ready"));
//! state.advance_send("Ready");
//! assert!(state.is_complete());
//! ```
//!
//! ## Multiparty
//!
//! ```ignore
//! use axiom::session::{GlobalType, GlobalOp, project, LocalType};
//!
//! // 3-party protocol: Buyer → Seller → Shipper → Buyer
//! let global = GlobalType::sequence(&[
//!     GlobalOp::Message { from: "Buyer", to: "Seller", label: "Order" },
//!     GlobalOp::Message { from: "Seller", to: "Shipper", label: "Ship" },
//!     GlobalOp::Message { from: "Shipper", to: "Buyer", label: "Delivered" },
//!     GlobalOp::End,
//! ]);
//!
//! // Project onto each role
//! let buyer_local = project(&global, "Buyer");
//! let seller_local = project(&global, "Seller");
//! let shipper_local = project(&global, "Shipper");
//! ```

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;

// ════════════════════════════════════════════════════════════════════════════
// PART 1: Binary Session Types
// ════════════════════════════════════════════════════════════════════════════

// ── Session operation ───────────────────────────────────────────────────────

/// A single operation in a binary session protocol.
///
/// Each operation describes one step: send, receive, choice, recursion, or
/// termination. A protocol is a sequence (or tree, for choices) of these.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionOp {
    /// Send a value of type `type_name`, then continue.
    Send {
        type_name: &'static str,
    },
    /// Receive a value of type `type_name`, then continue.
    Recv {
        type_name: &'static str,
    },
    /// Internal choice: the endpoint picks one of `branches`.
    /// Each branch is itself a `SessionType` (a sub-protocol).
    Select {
        branches: Vec<SessionType>,
    },
    /// External choice: the endpoint accepts one of `branches`.
    /// The peer decides which branch is taken.
    Choose {
        branches: Vec<SessionType>,
    },
    /// Recursive protocol: bind a variable `var` to `body`.
    /// The body may reference `var` via `SessionOp::Var`.
    Recurse {
        var: &'static str,
        body: Box<SessionType>,
    },
    /// Reference to a recursion variable (must be bound by an enclosing
    /// `Recurse`).
    Var {
        var: &'static str,
    },
    /// Terminate the session.
    End,
    /// No-op; continue to the next operation.
    Skip,
}

// ── Session type ────────────────────────────────────────────────────────────

/// A binary session type — a protocol describing the sequence of operations
/// on a port between two endpoints.
///
/// Stored as a sequence of `SessionOp`s. Choices (`Select`/`Choose`) nest
/// sub-protocols as `SessionType` values inside `SessionOp`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionType {
    ops: Vec<SessionOp>,
}

impl SessionType {
    /// Create a session type from a sequence of operations.
    pub fn sequence(ops: &[SessionOp]) -> Self {
        Self { ops: ops.to_vec() }
    }

    /// Empty session (equivalent to `end`).
    pub fn empty() -> Self {
        Self { ops: vec![] }
    }

    /// A single-operation session.
    pub fn single(op: SessionOp) -> Self {
        Self { ops: vec![op] }
    }

    /// The `end` session type.
    pub fn end() -> Self {
        Self { ops: vec![SessionOp::End] }
    }

    /// Chain another session type after this one.
    pub fn then(mut self, other: SessionType) -> Self {
        self.ops.extend(other.ops);
        self
    }

    /// Append a single operation.
    pub fn push(mut self, op: SessionOp) -> Self {
        self.ops.push(op);
        self
    }

    /// All operations in this session type (in order).
    pub fn ops(&self) -> &[SessionOp] {
        &self.ops
    }

    /// Whether this session type contains any operations.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

// ── Session state (runtime automaton) ───────────────────────────────────────

/// Runtime state of a binary session — tracks progress through a `SessionType`.
///
/// The runtime advances the state by calling `advance_send` / `advance_recv`
/// each time a value crosses the port. If the operation doesn't match the
/// protocol, the advance fails and the session enters an error state.
///
/// # Thread safety
///
/// `SessionState` is `Send + Sync` because it uses no interior mutability
/// — the caller holds a `&mut Self` to advance. For shared state across
/// threads, wrap in `Mutex<SessionState>` or `RwLock<SessionState>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionState {
    protocol: SessionType,
    /// Current position in the protocol's op sequence.
    /// Points to the next operation to perform.
    cursor: usize,
    /// Whether the session has terminated (reached `End` or exhausted ops).
    completed: bool,
    /// Whether the session is in an error state (protocol violation).
    error: bool,
}

impl SessionState {
    /// Create a new session state at the beginning of the protocol.
    pub fn new(protocol: &SessionType) -> Self {
        Self {
            protocol: protocol.clone(),
            cursor: 0,
            completed: protocol.ops.is_empty()
                || matches!(protocol.ops.first(), Some(SessionOp::End)),
            error: false,
        }
    }

    /// Current operation, if any.
    pub fn current_op(&self) -> Option<&SessionOp> {
        if self.error || self.completed {
            return None;
        }
        self.protocol.ops.get(self.cursor)
    }

    /// Whether the session can currently send a value of `type_name`.
    pub fn can_send(&self, type_name: &str) -> bool {
        match self.current_op() {
            Some(SessionOp::Send { type_name: tn }) => *tn == type_name,
            _ => false,
        }
    }

    /// Whether the session can currently receive a value of `type_name`.
    pub fn can_recv(&self, type_name: &str) -> bool {
        match self.current_op() {
            Some(SessionOp::Recv { type_name: tn }) => *tn == type_name,
            _ => false,
        }
    }

    /// Advance the session by sending a value of `type_name`.
    /// Returns `true` if the advance was valid, `false` if it violated
    /// the protocol (putting the session into error state).
    pub fn advance_send(&mut self, type_name: &str) -> bool {
        if !self.can_send(type_name) {
            self.error = true;
            return false;
        }
        self.cursor += 1;
        self.check_completion();
        true
    }

    /// Advance the session by receiving a value of `type_name`.
    /// Returns `true` if the advance was valid, `false` otherwise.
    pub fn advance_recv(&mut self, type_name: &str) -> bool {
        if !self.can_recv(type_name) {
            self.error = true;
            return false;
        }
        self.cursor += 1;
        self.check_completion();
        true
    }

    /// Whether the session has completed (reached `End` or exhausted ops).
    pub fn is_complete(&self) -> bool {
        self.completed
    }

    /// Whether the session is in an error state (protocol violation).
    pub fn is_error(&self) -> bool {
        self.error
    }

    /// Reset the session to the beginning of the protocol.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.completed = self.protocol.ops.is_empty()
            || matches!(self.protocol.ops.first(), Some(SessionOp::End));
        self.error = false;
    }

    /// Progress as a fraction in `[0.0, 1.0]`.
    pub fn progress(&self) -> f64 {
        if self.protocol.ops.is_empty() {
            return 1.0;
        }
        (self.cursor as f64) / (self.protocol.ops.len() as f64)
    }

    fn check_completion(&mut self) {
        match self.protocol.ops.get(self.cursor) {
            None => self.completed = true,
            Some(SessionOp::End) => self.completed = true,
            Some(SessionOp::Skip) => {
                self.cursor += 1;
                self.check_completion();
            }
            _ => {}
        }
    }
}

// ── SessionProtocol trait (compile-time protocol adherence) ─────────────────

/// A trait for types that carry a compile-time binary session protocol.
///
/// Implementing this trait allows a Machine's port to declare its protocol
/// statically. The runtime can then verify that the peer's protocol is
/// **dual** (the mirror image: sends become recvs, recvs become sends).
///
/// # Duality
///
/// Two session types S and T are dual (S ⊥ T) if:
/// - `!T.S` ⊥ `?T.T'`  (send dual to recv)
/// - `S₁ + S₂` ⊥ `T₁ & T₂`  (internal choice dual to external choice)
/// - `end` ⊥ `end`
/// - `μX.S` ⊥ `μX.T`  (recursion dual)
pub trait SessionProtocol: Send + Sync + 'static {
    /// The session type descriptor for this port.
    fn protocol() -> SessionType;

    /// Check whether this protocol is dual to another.
    /// Two ports can be linked iff their protocols are dual.
    fn is_dual_to<P: SessionProtocol>() -> bool {
        is_dual(&Self::protocol(), &P::protocol())
    }
}

// ── Duality check ───────────────────────────────────────────────────────────

/// Check whether two binary session types are dual.
///
/// This is the core compatibility check for session-typed ports: two ports
/// can be linked iff their session types are dual.
///
/// # Rules
///
/// - `Send{T}.S` is dual to `Recv{T}.T` iff S is dual to T
/// - `Recv{T}.S` is dual to `Send{T}.T` iff S is dual to T
/// - `End` is dual to `End`
/// - `Skip` is dual to `Skip` (and skipped)
/// - Empty is dual to empty
/// - All other combinations are not dual
pub fn is_dual(a: &SessionType, b: &SessionType) -> bool {
    let mut ai = a.ops.iter().peekable();
    let mut bi = b.ops.iter().peekable();

    loop {
        // Skip `Skip` operations on both sides.
        while matches!(ai.peek(), Some(SessionOp::Skip)) {
            ai.next();
        }
        while matches!(bi.peek(), Some(SessionOp::Skip)) {
            bi.next();
        }

        match (ai.peek(), bi.peek()) {
            (None, None) => return true,
            (Some(SessionOp::End), None) => return true,
            (None, Some(SessionOp::End)) => return true,
            (Some(SessionOp::End), Some(SessionOp::End)) => return true,

            (Some(SessionOp::Send { type_name: ta }), Some(SessionOp::Recv { type_name: tb })) => {
                if ta != tb {
                    return false;
                }
                ai.next();
                bi.next();
            }
            (Some(SessionOp::Recv { type_name: ta }), Some(SessionOp::Send { type_name: tb })) => {
                if ta != tb {
                    return false;
                }
                ai.next();
                bi.next();
            }
            // Internal choice is dual to external choice: the branch counts
            // must match and each branch pair must be dual.
            (Some(SessionOp::Select { branches: ba }), Some(SessionOp::Choose { branches: bb })) => {
                if ba.len() != bb.len() {
                    return false;
                }
                for (x, y) in ba.iter().zip(bb.iter()) {
                    if !is_dual(x, y) {
                        return false;
                    }
                }
                ai.next();
                bi.next();
            }
            (Some(SessionOp::Choose { branches: ba }), Some(SessionOp::Select { branches: bb })) => {
                if ba.len() != bb.len() {
                    return false;
                }
                for (x, y) in ba.iter().zip(bb.iter()) {
                    if !is_dual(x, y) {
                        return false;
                    }
                }
                ai.next();
                bi.next();
            }
            // Recursive types are dual iff the same variable is bound and
            // the bodies are dual (isomorphic recursion — no unfolding).
            (Some(SessionOp::Recurse { var: va, body: ba }), Some(SessionOp::Recurse { var: vb, body: bb })) => {
                if va != vb {
                    return false;
                }
                if !is_dual(ba, bb) {
                    return false;
                }
                ai.next();
                bi.next();
            }
            // Variable references are dual iff they name the same binding.
            (Some(SessionOp::Var { var: va }), Some(SessionOp::Var { var: vb })) => {
                if va != vb {
                    return false;
                }
                ai.next();
                bi.next();
            }
            _ => return false,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// PART 2: Multiparty Session Types (MPST)
// ════════════════════════════════════════════════════════════════════════════

// ── Role ────────────────────────────────────────────────────────────────────

/// A participant in a multiparty session.
///
/// Roles are identified by name (e.g., "Buyer", "Seller", "Shipper").
/// Each role has exactly one local endpoint type after projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Role(pub &'static str);

impl Role {
    pub fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn name(&self) -> &'static str {
        self.0
    }
}

impl core::fmt::Display for Role {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Global operation ────────────────────────────────────────────────────────

/// A single operation in a global (multiparty) session type.
///
/// Describes the choreography of all participants. Each `Message` operation
/// specifies a sender, receiver, and label. The global type is projected
/// onto each role to obtain a local type.
#[derive(Debug, Clone, PartialEq)]
pub enum GlobalOp {
    /// A message from role `from` to role `to` with label `label`.
    /// The label identifies the message type (analogous to `type_name`
    /// in binary session types).
    Message {
        from: &'static str,
        to: &'static str,
        label: &'static str,
    },
    /// Choice between two global behaviors.
    /// The first role in `selector` decides which branch to take.
    Choice {
        selector: &'static str,
        branches: Vec<GlobalType>,
    },
    /// Recursive global type: bind a variable `var` to `body`.
    Recurse {
        var: &'static str,
        body: Box<GlobalType>,
    },
    /// Reference to a recursion variable.
    Var {
        var: &'static str,
    },
    /// Terminate the global session.
    End,
    /// No-op; continue to the next operation.
    Skip,
}

// ── Global type ─────────────────────────────────────────────────────────────

/// A global session type — the choreography of a multiparty protocol.
///
/// Describes the interactions among all participants from a global
/// perspective. Each role's local type is obtained by [`project`]ing
/// the global type onto that role.
///
/// # Properties
///
/// - **Communication safety**: messages are always sent to a recipient
///   that expects them.
/// - **Progress**: if all participants follow their local types, the
///   protocol cannot deadlock (no participant waits forever).
/// - **Session fidelity**: the runtime can check that each message
///   conforms to the projected local type.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalType {
    ops: Vec<GlobalOp>,
}

impl GlobalType {
    /// Create a global type from a sequence of operations.
    pub fn sequence(ops: &[GlobalOp]) -> Self {
        Self { ops: ops.to_vec() }
    }

    /// Empty global type (equivalent to `end`).
    pub fn empty() -> Self {
        Self { ops: vec![] }
    }

    /// The `end` global type.
    pub fn end() -> Self {
        Self { ops: vec![GlobalOp::End] }
    }

    /// Chain another global type after this one.
    pub fn then(mut self, other: GlobalType) -> Self {
        self.ops.extend(other.ops);
        self
    }

    /// Append a single operation.
    pub fn push(mut self, op: GlobalOp) -> Self {
        self.ops.push(op);
        self
    }

    /// All operations in this global type (in order).
    pub fn ops(&self) -> &[GlobalOp] {
        &self.ops
    }

    /// All roles that participate in this global type.
    pub fn roles(&self) -> Vec<Role> {
        let mut roles = crate::compat::HashSet::new();
        for op in &self.ops {
            match op {
                GlobalOp::Message { from, to, .. } => {
                    roles.insert(Role(from));
                    roles.insert(Role(to));
                }
                GlobalOp::Choice { selector, branches } => {
                    roles.insert(Role(selector));
                    for branch in branches {
                        for r in branch.roles() {
                            roles.insert(r);
                        }
                    }
                }
                GlobalOp::Recurse { body, .. } => {
                    for r in body.roles() {
                        roles.insert(r);
                    }
                }
                _ => {}
            }
        }
        roles.into_iter().collect()
    }
}

// ── Local type (projection result) ──────────────────────────────────────────

/// A local session type — the view of a global protocol from one role's
/// perspective.
///
/// Obtained by projecting a [`GlobalType`] onto a specific role via
/// [`project()`]. Each role in a multiparty session has exactly one
/// local type.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalType {
    ops: Vec<LocalOp>,
}

impl LocalType {
    /// Create a local type from a sequence of operations.
    pub fn sequence(ops: &[LocalOp]) -> Self {
        Self { ops: ops.to_vec() }
    }

    /// Empty local type.
    pub fn empty() -> Self {
        Self { ops: vec![] }
    }

    /// The `end` local type.
    pub fn end() -> Self {
        Self { ops: vec![LocalOp::End] }
    }

    /// All operations in this local type (in order).
    pub fn ops(&self) -> &[LocalOp] {
        &self.ops
    }

    /// Whether this local type contains any operations.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Convert to a binary `SessionType` for use with `SessionState`.
    ///
    /// Each `Send`/`Recv` becomes the corresponding binary operation.
    /// The peer role information is discarded — the binary form is
    /// sufficient for runtime state tracking.
    pub fn to_binary(&self) -> SessionType {
        let binary_ops: Vec<SessionOp> = self.ops.iter().map(|op| match op {
            LocalOp::Send { label, .. } => SessionOp::Send { type_name: label },
            LocalOp::Recv { label, .. } => SessionOp::Recv { type_name: label },
            LocalOp::Select { branches } => SessionOp::Select {
                branches: branches.iter().map(|b| b.to_binary()).collect(),
            },
            LocalOp::Choose { branches } => SessionOp::Choose {
                branches: branches.iter().map(|b| b.to_binary()).collect(),
            },
            LocalOp::Recurse { var, body } => SessionOp::Recurse {
                var,
                body: Box::new(body.to_binary()),
            },
            LocalOp::Var { var } => SessionOp::Var { var },
            LocalOp::End => SessionOp::End,
            LocalOp::Skip => SessionOp::Skip,
        }).collect();
        SessionType::sequence(&binary_ops)
    }
}

/// A single operation in a local session type.
#[derive(Debug, Clone, PartialEq)]
pub enum LocalOp {
    /// Send a message with `label` to `to`.
    Send {
        to: &'static str,
        label: &'static str,
    },
    /// Receive a message with `label` from `from`.
    Recv {
        from: &'static str,
        label: &'static str,
    },
    /// Internal choice: this role picks one of `branches` to continue
    /// (projection of a global `Choice` where this role is the selector).
    Select {
        branches: Vec<LocalType>,
    },
    /// External choice: this role accepts whichever branch the peer picks
    /// (projection of a global `Choice` where a *different* role selects).
    Choose {
        branches: Vec<LocalType>,
    },
    /// Recursive local type: bind `var` to `body`; `body` may reference
    /// `var` via `LocalOp::Var` (projection of a global `Recurse`).
    Recurse {
        var: &'static str,
        body: Box<LocalType>,
    },
    /// Reference to a recursion variable (must be bound by an enclosing
    /// `Recurse`).
    Var {
        var: &'static str,
    },
    /// Terminate the local session.
    End,
    /// No-op; continue to the next operation.
    Skip,
}

// ── Projection ──────────────────────────────────────────────────────────────

/// Project a global type onto a specific role to obtain the local type.
///
/// For each global operation:
/// - `Message { from, to, label }`:
///   - If `role == from` → `LocalOp::Send { to, label }`
///   - If `role == to` → `LocalOp::Recv { from, label }`
///   - Otherwise → `Skip` (this role is not involved)
/// - `Choice { selector, branches }`:
///   - If `role == selector` → `Select { branches: [proj_i] }` (internal
///     choice: this role picks)
///   - Otherwise → `Choose { branches: [proj_i] }` (external choice: this
///     role accepts the peer's pick)
///   - If the role is inert in every branch → `Skip` (collapsed)
/// - `Recurse { var, body }` → `Recurse { var, body: project(body) }`
///   (full body projection — the recursion structure is preserved); if the
///   role is inert in the body → `Skip` (collapsed)
/// - `Var { var }` → `Var { var }` (the reference survives projection)
/// - `End` → `End`
/// - `Skip` → `Skip`
///
/// # Example
///
/// ```ignore
/// use axiom::session::{GlobalType, GlobalOp, project};
///
/// let global = GlobalType::sequence(&[
///     GlobalOp::Message { from: "Buyer", to: "Seller", label: "Order" },
///     GlobalOp::Message { from: "Seller", to: "Buyer", label: "Confirm" },
///     GlobalOp::End,
/// ]);
///
/// let buyer = project(&global, "Buyer");
/// // buyer.ops() = [Send{to:"Seller",label:"Order"}, Recv{from:"Seller",label:"Confirm"}, End]
/// ```
pub fn project(global: &GlobalType, role: &str) -> LocalType {
    let local_ops: Vec<LocalOp> = global.ops.iter().map(|op| match op {
        GlobalOp::Message { from, to, label } => {
            if *from == role {
                LocalOp::Send { to, label }
            } else if *to == role {
                LocalOp::Recv { from, label }
            } else {
                LocalOp::Skip
            }
        }
        GlobalOp::End => LocalOp::End,
        GlobalOp::Skip => LocalOp::Skip,
        GlobalOp::Choice { selector, branches } => {
            // Project every branch onto this role, preserving the full
            // branching structure (A-completeness of MPST projection).
            let proj: Vec<LocalType> = branches.iter().map(|b| project(b, role)).collect();
            if proj.iter().all(is_inert) {
                // The role does not participate in any branch — collapse.
                LocalOp::Skip
            } else if *selector == role {
                LocalOp::Select { branches: proj }
            } else {
                LocalOp::Choose { branches: proj }
            }
        }
        GlobalOp::Recurse { var, body } => {
            // Project the full body, preserving recursion structure.
            let proj = project(body, role);
            if is_inert(&proj) {
                LocalOp::Skip
            } else {
                LocalOp::Recurse {
                    var,
                    body: Box::new(proj),
                }
            }
        }
        GlobalOp::Var { var } => LocalOp::Var { var },
    }).collect();

    LocalType { ops: local_ops }
}

/// Whether a local type is *inert* for a role: every op is `Skip` (or the
/// type is empty) — the role has nothing to do on this path.
fn is_inert(lt: &LocalType) -> bool {
    lt.ops.iter().all(|op| matches!(op, LocalOp::Skip))
}

// ── MPST compatibility check ────────────────────────────────────────────────

/// Check whether a set of local types is consistent with a global type.
///
/// This verifies that projecting the global type onto each role produces
/// local types that are mutually consistent — i.e., every send has a
/// matching recv on the peer role.
///
/// # Algorithm
///
/// 1. Collect all roles from the global type (O(n)).
/// 2. Project the global type onto each role **once** (O(R × n), R = roles).
/// 3. For each `Message { from, to, label }`, look up the pre-computed
///    projections for `from` and `to` (O(1) per lookup) and verify the
///    sender has `Send { to, label }` and the receiver has
///    `Recv { from, label }`.
///
/// **Complexity**: O(R × n + n × L) where L ≤ n is the average local type
/// length. Since R is typically small (2–5), this is effectively O(n).
/// The previous implementation re-projected for every message, giving O(n²).
pub fn is_consistent(global: &GlobalType) -> bool {
    // Step 1: Collect all roles and project once.
    let roles = global.roles();
    let projections: Vec<(Role, LocalType)> = roles
        .iter()
        .map(|role| (*role, project(global, role.0)))
        .collect();

    // Helper: find a role's projection by name (linear scan; R is small).
    let find_proj = |role: &str| {
        projections.iter().find(|(r, _)| r.0 == role).map(|(_, lt)| lt)
    };

    // Step 2: For each Message (recursively — including inside Choice
    // branches and Recurse bodies), verify sender and receiver projections
    // match.
    let mut ok = true;
    for_each_message(&global.ops, &mut |op| {
        if let GlobalOp::Message { from, to, label } = op {
            let sender_local = match find_proj(from) {
                Some(lt) => lt,
                None => {
                    ok = false;
                    return;
                }
            };
            let receiver_local = match find_proj(to) {
                Some(lt) => lt,
                None => {
                    ok = false;
                    return;
                }
            };

            // Check sender has a Send op matching this message (recursively
            // — the op may be nested inside Select/Choose/Recurse).
            let sender_ok = local_has(
                sender_local,
                &LocalOp::Send { to, label },
            );
            // Check receiver has a Recv op matching this message.
            let receiver_ok = local_has(
                receiver_local,
                &LocalOp::Recv { from, label },
            );

            if !sender_ok || !receiver_ok {
                ok = false;
            }
        }
    });
    ok
}

/// Whether a local type contains the given op, descending into choice
/// branches and recursion bodies.
fn local_has(local: &LocalType, target: &LocalOp) -> bool {
    local.ops.iter().any(|lop| {
        if lop == target {
            true
        } else {
            match lop {
                LocalOp::Select { branches } | LocalOp::Choose { branches } => {
                    branches.iter().any(|b| local_has(b, target))
                }
                LocalOp::Recurse { body, .. } => local_has(body, target),
                _ => false,
            }
        }
    })
}

/// Visit every `Message` in a global op sequence, descending into `Choice`
/// branches and `Recurse` bodies (recursion-aware consistency checking).
fn for_each_message<'a>(ops: &'a [GlobalOp], f: &mut impl FnMut(&'a GlobalOp)) {
    for op in ops {
        match op {
            GlobalOp::Message { .. } => f(op),
            GlobalOp::Choice { branches, .. } => {
                for b in branches {
                    for_each_message(&b.ops, f);
                }
            }
            GlobalOp::Recurse { body, .. } => for_each_message(&body.ops, f),
            _ => {}
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// PART 3: Errors
// ════════════════════════════════════════════════════════════════════════════

/// Errors that can occur during session type validation.
#[derive(Debug)]
pub enum SessionError {
    /// The operation violated the protocol.
    ProtocolViolation {
        expected: &'static str,
        actual: &'static str,
    },
    /// The session is in an error state and cannot advance.
    SessionInError,
    /// The session has already completed.
    SessionComplete,
    /// Two binary session types are not dual (cannot be linked).
    NotDual {
        a: SessionType,
        b: SessionType,
    },
    /// A multiparty global type is inconsistent (projections don't match).
    InconsistentGlobal {
        message: String,
    },
    /// A role was not found in the global type.
    UnknownRole {
        role: &'static str,
    },
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ProtocolViolation { expected, actual } => {
                write!(f, "protocol violation: expected {}, got {}", expected, actual)
            }
            Self::SessionInError => write!(f, "session is in error state"),
            Self::SessionComplete => write!(f, "session has already completed"),
            Self::NotDual { a, b } => {
                write!(f, "session types are not dual: {:?} vs {:?}", a, b)
            }
            Self::InconsistentGlobal { message } => {
                write!(f, "inconsistent global type: {}", message)
            }
            Self::UnknownRole { role } => {
                write!(f, "unknown role: {}", role)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for SessionError {}

// ════════════════════════════════════════════════════════════════════════════
// PART 4: Convenience — wrap LocalType as a PortDecl session
// ════════════════════════════════════════════════════════════════════════════

/// Convert a `LocalType` to a `SessionType` suitable for `PortDecl::with_session()`.
///
/// This is the bridge between MPST (which produces `LocalType`s) and the
/// port system (which uses `SessionType`). The peer role information is
/// discarded — the binary form is sufficient for runtime state tracking.
impl From<LocalType> for SessionType {
    fn from(local: LocalType) -> Self {
        local.to_binary()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// PART 5: Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_dual ────────────────────────────────────────────────────────────

    #[test]
    fn is_dual_send_recv_same_type() {
        let a = SessionType::single(SessionOp::Send { type_name: "T" });
        let b = SessionType::single(SessionOp::Recv { type_name: "T" });
        assert!(is_dual(&a, &b));
    }

    #[test]
    fn is_dual_recv_send_same_type() {
        let a = SessionType::single(SessionOp::Recv { type_name: "T" });
        let b = SessionType::single(SessionOp::Send { type_name: "T" });
        assert!(is_dual(&a, &b));
    }

    #[test]
    fn is_dual_send_recv_different_type() {
        let a = SessionType::single(SessionOp::Send { type_name: "A" });
        let b = SessionType::single(SessionOp::Recv { type_name: "B" });
        assert!(!is_dual(&a, &b));
    }

    #[test]
    fn is_dual_send_send() {
        let a = SessionType::single(SessionOp::Send { type_name: "T" });
        let b = SessionType::single(SessionOp::Send { type_name: "T" });
        assert!(!is_dual(&a, &b));
    }

    #[test]
    fn is_dual_end_end() {
        let a = SessionType::end();
        let b = SessionType::end();
        assert!(is_dual(&a, &b));
    }

    #[test]
    fn is_dual_end_empty() {
        // End ⊥ empty (and empty ⊥ End) — loose terminal matching.
        let a = SessionType::end();
        let b = SessionType::empty();
        assert!(is_dual(&a, &b));
        assert!(is_dual(&b, &a));
        // empty ⊥ empty
        assert!(is_dual(&SessionType::empty(), &SessionType::empty()));
    }

    #[test]
    fn is_dual_select_choose_matching() {
        // Select ⊥ Choose: same branch count, each branch pair dual.
        let a = SessionType::single(SessionOp::Select {
            branches: vec![SessionType::single(SessionOp::Send { type_name: "A" })],
        });
        let b = SessionType::single(SessionOp::Choose {
            branches: vec![SessionType::single(SessionOp::Recv { type_name: "A" })],
        });
        assert!(is_dual(&a, &b));
        // Symmetric: Choose ⊥ Select.
        assert!(is_dual(&b, &a));
    }

    #[test]
    fn is_dual_select_choose_mismatched_count() {
        let a = SessionType::single(SessionOp::Select {
            branches: vec![
                SessionType::single(SessionOp::Send { type_name: "A" }),
                SessionType::single(SessionOp::Send { type_name: "B" }),
            ],
        });
        let b = SessionType::single(SessionOp::Choose {
            branches: vec![SessionType::single(SessionOp::Recv { type_name: "A" })],
        });
        assert!(!is_dual(&a, &b));
    }

    #[test]
    fn is_dual_recurse_same_var() {
        // Recurse{var, body} ⊥ Recurse{var, dual_body}: same var, dual bodies.
        let a = SessionType::single(SessionOp::Recurse {
            var: "X",
            body: Box::new(SessionType::single(SessionOp::Send { type_name: "A" })),
        });
        let b = SessionType::single(SessionOp::Recurse {
            var: "X",
            body: Box::new(SessionType::single(SessionOp::Recv { type_name: "A" })),
        });
        assert!(is_dual(&a, &b));
    }

    #[test]
    fn is_dual_recurse_different_var() {
        let a = SessionType::single(SessionOp::Recurse {
            var: "X",
            body: Box::new(SessionType::single(SessionOp::Send { type_name: "A" })),
        });
        let b = SessionType::single(SessionOp::Recurse {
            var: "Y",
            body: Box::new(SessionType::single(SessionOp::Recv { type_name: "A" })),
        });
        assert!(!is_dual(&a, &b));
    }

    #[test]
    fn is_dual_sequence() {
        // [Send{A}, Recv{B}] ⊥ [Recv{A}, Send{B}] — element-wise duality.
        let a = SessionType::sequence(&[
            SessionOp::Send { type_name: "A" },
            SessionOp::Recv { type_name: "B" },
        ]);
        let b = SessionType::sequence(&[
            SessionOp::Recv { type_name: "A" },
            SessionOp::Send { type_name: "B" },
        ]);
        assert!(is_dual(&a, &b));
    }

    #[test]
    fn is_dual_skip_transparent() {
        // Skip is transparent: [Skip, Send{A}] ⊥ [Recv{A}].
        let a = SessionType::sequence(&[
            SessionOp::Skip,
            SessionOp::Send { type_name: "T" },
        ]);
        let b = SessionType::single(SessionOp::Recv { type_name: "T" });
        assert!(is_dual(&a, &b));
    }

    // ── SessionState ───────────────────────────────────────────────────────

    #[test]
    fn session_state_advance_send_ok() {
        let proto = SessionType::single(SessionOp::Send { type_name: "T" });
        let mut s = SessionState::new(&proto);
        assert!(s.can_send("T"));
        assert!(s.advance_send("T"));
        // Single op consumed → exhausted → complete, no error.
        assert!(s.is_complete());
        assert!(!s.is_error());
    }

    #[test]
    fn session_state_advance_send_wrong_type() {
        let proto = SessionType::single(SessionOp::Send { type_name: "T" });
        let mut s = SessionState::new(&proto);
        assert!(!s.advance_send("X"));
        assert!(s.is_error());
        assert!(!s.is_complete());
    }

    #[test]
    fn session_state_advance_recv_ok() {
        let proto = SessionType::single(SessionOp::Recv { type_name: "T" });
        let mut s = SessionState::new(&proto);
        assert!(s.can_recv("T"));
        assert!(s.advance_recv("T"));
        assert!(s.is_complete());
        assert!(!s.is_error());
    }

    #[test]
    fn session_state_advance_when_complete() {
        let proto = SessionType::end();
        let mut s = SessionState::new(&proto);
        assert!(s.is_complete());
        // Already complete — cannot advance, enters error.
        assert!(!s.advance_send("T"));
        assert!(s.is_error());
    }

    #[test]
    fn session_state_reset() {
        let proto = SessionType::sequence(&[
            SessionOp::Send { type_name: "T" },
            SessionOp::Send { type_name: "U" },
        ]);
        let mut s = SessionState::new(&proto);
        assert!(s.advance_send("T"));
        assert_eq!(s.progress(), 0.5);

        // Reset clears cursor and recomputes completion.
        s.reset();
        assert_eq!(s.progress(), 0.0);
        assert!(!s.is_complete());
        assert!(!s.is_error());

        // Reset also clears an error state.
        assert!(!s.advance_send("WRONG"));
        assert!(s.is_error());
        s.reset();
        assert!(!s.is_error());
    }

    #[test]
    fn session_state_progress() {
        let proto = SessionType::sequence(&[
            SessionOp::Send { type_name: "A" },
            SessionOp::Send { type_name: "B" },
            SessionOp::Send { type_name: "C" },
        ]);
        let mut s = SessionState::new(&proto);
        assert_eq!(s.progress(), 0.0);
        assert!(s.advance_send("A"));
        // 1 of 3 ops consumed ≈ 0.333.
        assert!((s.progress() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn session_state_skip_auto_completes() {
        // [Send{A}, Skip, End]: advancing the Send auto-skips Skip to reach End.
        let proto = SessionType::sequence(&[
            SessionOp::Send { type_name: "T" },
            SessionOp::Skip,
            SessionOp::End,
        ]);
        let mut s = SessionState::new(&proto);
        assert!(s.advance_send("T"));
        assert!(s.is_complete());
        assert!(!s.is_error());
    }

    // ── project ─────────────────────────────────────────────────────────────

    #[test]
    fn project_sender() {
        let global = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "msg",
        }]);
        let local = project(&global, "A");
        assert_eq!(local.ops(), &[LocalOp::Send { to: "B", label: "msg" }]);
    }

    #[test]
    fn project_receiver() {
        let global = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "msg",
        }]);
        let local = project(&global, "B");
        assert_eq!(local.ops(), &[LocalOp::Recv { from: "A", label: "msg" }]);
    }

    #[test]
    fn project_unrelated_role() {
        let global = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "msg",
        }]);
        let local = project(&global, "C");
        assert_eq!(local.ops(), &[LocalOp::Skip]);
    }

    #[test]
    fn project_choice_selector() {
        let branch1 = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "x",
        }]);
        let branch2 = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "y",
        }]);
        let global = GlobalType::empty().push(GlobalOp::Choice {
            selector: "A",
            branches: vec![branch1, branch2],
        });
        let local = project(&global, "A");
        // Selector → internal Select with both branches projected.
        assert_eq!(local.ops().len(), 1);
        let ops = local.ops();
        let LocalOp::Select { branches } = &ops[0] else {
            panic!("expected Select variant");
        };
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].ops(), &[LocalOp::Send { to: "B", label: "x" }]);
        assert_eq!(branches[1].ops(), &[LocalOp::Send { to: "B", label: "y" }]);
    }

    #[test]
    fn project_choice_non_selector() {
        let branch1 = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "x",
        }]);
        let branch2 = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "y",
        }]);
        let global = GlobalType::empty().push(GlobalOp::Choice {
            selector: "A",
            branches: vec![branch1, branch2],
        });
        let local = project(&global, "B");
        // Non-selector → external Choose.
        assert_eq!(local.ops().len(), 1);
        let ops = local.ops();
        let LocalOp::Choose { branches } = &ops[0] else {
            panic!("expected Choose variant");
        };
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].ops(), &[LocalOp::Recv { from: "A", label: "x" }]);
        assert_eq!(branches[1].ops(), &[LocalOp::Recv { from: "A", label: "y" }]);
    }

    #[test]
    fn project_inert_choice_collapses() {
        // Role C participates in no branch → inert → collapses to Skip.
        let branch1 = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "x",
        }]);
        let global = GlobalType::empty().push(GlobalOp::Choice {
            selector: "A",
            branches: vec![branch1],
        });
        let local = project(&global, "C");
        assert_eq!(local.ops(), &[LocalOp::Skip]);
    }

    // ── is_consistent ──────────────────────────────────────────────────────

    #[test]
    fn is_consistent_simple() {
        let global = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "B",
            label: "msg",
        }]);
        assert!(is_consistent(&global));
    }

    #[test]
    fn is_consistent_missing_receiver() {
        // `is_consistent` re-derives local types via `project()`, so every
        // well-formed Message yields a matching Send on `from` and Recv on `to`.
        // The one well-formed-looking case the checker flags as inconsistent is
        // a self-message (from == to): the `from == role` arm wins in
        // `project`, so the role's local type carries `Send` but no `Recv`,
        // and the receiver-side check fails.
        let global = GlobalType::sequence(&[GlobalOp::Message {
            from: "A",
            to: "A",
            label: "x",
        }]);
        assert!(!is_consistent(&global));
    }

    #[test]
    fn is_consistent_three_party() {
        // A → B → C chain: each message has a matching sender/receiver pair.
        let global = GlobalType::sequence(&[
            GlobalOp::Message { from: "A", to: "B", label: "m1" },
            GlobalOp::Message { from: "B", to: "C", label: "m2" },
            GlobalOp::End,
        ]);
        assert!(is_consistent(&global));
    }
}
