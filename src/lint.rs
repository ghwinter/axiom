//! **Maturity: tool** (a development-time tool, reinforced per the unified convention).
//!
//! Architecture lint rules — the anti-narrowing axioms as executable checks.
//!
//! axiom's philosophy documents (philosophy.md, architecture.md) establish
//! axioms that are easy to violate silently:
//!
//! - **Observability is a contract** — a system without any [`FlowKind::Observe`]
//!   port cannot be observed; the graph hides its state.
//! - **Physical decisions must be explicit** — a blueprint where every
//!   [`crate::resource::MachinePhysicalSpec`] is left at `default()` has not made its physical
//!   decisions visible; the runtime will silently choose for you.
//! - **Link kinds express traffic shape** — using one `LinkKind` for every
//!   link ignores whether a channel should block, drop, or overwrite.
//! - **Pure functions are a first-class layer** — a graph with machines only
//!   and no `FuncBinding`s narrows the pure/stateful split to "everything is
//!   stateful".
//!
//! [`lint`] runs these checks against a [`DynamicTopology`] and returns a
//! [`ValidationReport`] (reusing the structured violation type from
//! [`DynamicTopology::validate_report`]): hard violations go to `violations`,
//! advisory findings to `warnings`. Unlike `validate_deep`, lint rules are
//! **heuristics over the blueprint**, not correctness checks — a clean lint
//! report is not required for a valid deployment, but each finding names an
//! axiom the blueprint is drifting away from.
//!
//! Each rule is data (see [`RULES`]): a stable `id`, a `severity`, a
//! human `description`, and a pure check function. AI tooling can run a
//! single rule by id or the whole set, and map `RuleViolation::rule_id`
//! straight back to the documented axiom.

use crate::compat::HashMap;
use crate::deploy::{DynamicTopology, RuleViolation, ValidationReport};
use crate::flow::FlowKind;
use crate::link::LinkKind;
use crate::port::PortSchema;
use crate::resource::ExecutionHint;

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::vec;
use alloc::vec::Vec;

/// Severity of a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Violates a hard contract — fix before deploy.
    Error,
    /// Drifts from a documented axiom — strongly consider fixing.
    Warning,
    /// Advisory — may be entirely appropriate for small systems.
    Info,
}

/// A single lint rule: metadata + a pure check function.
#[derive(Debug, Clone, Copy)]
pub struct LintRule {
    /// Stable identifier, e.g. `"no-observation"`. Used as `RuleViolation::rule_id`.
    pub id: &'static str,
    pub severity: Severity,
    /// One-line description of the axiom this rule enforces.
    pub description: &'static str,
    /// Pure check: returns the violations this rule finds.
    pub check: fn(&DynamicTopology, Option<&HashMap<&str, PortSchema>>) -> Vec<RuleViolation>,
}

/// The built-in lint rule set (anti-narrowing axioms).
pub const RULES: &[LintRule] = &[
    LintRule {
        id: "orphan-machine",
        severity: Severity::Warning,
        description: "machine with no incoming or outgoing links — an island in the graph",
        check: check_orphan_machine,
    },
    LintRule {
        id: "unused-output-port",
        severity: Severity::Warning,
        description: "declared output port is never consumed by any link — the graph hides a produced value",
        check: check_unused_output_port,
    },
    LintRule {
        id: "no-observation",
        severity: Severity::Warning,
        description: "no Observe-flow port anywhere — the system cannot be observed (observability is a contract)",
        check: check_no_observation,
    },
    LintRule {
        id: "no-control",
        severity: Severity::Info,
        description: "no Control-flow port anywhere — the system cannot be steered at runtime",
        check: check_no_control,
    },
    LintRule {
        id: "uniform-link-kind",
        severity: Severity::Info,
        description: "every link uses the same LinkKind — carrier choice does not reflect traffic shape",
        check: check_uniform_link_kind,
    },
    LintRule {
        id: "default-physical",
        severity: Severity::Warning,
        description: "every MachinePhysicalSpec is left at default() — physical decisions were not made explicit",
        check: check_default_physical,
    },
    LintRule {
        id: "no-funcs",
        severity: Severity::Info,
        description: "no FuncBinding — the pure-function layer is unused; everything is a stateful machine",
        check: check_no_funcs,
    },
    LintRule {
        id: "no-moore-headroom",
        severity: Severity::Info,
        description: "acyclic topology with no Moore machine — adding a feedback loop later will fail validation",
        check: check_no_moore_headroom,
    },
];

/// Run the full lint rule set against a spec.
///
/// `schemas` (machine name → [`PortSchema`]) enables the port-level rules
/// (`unused-output-port`, `no-observation`, `no-control`). Pass `None` to
/// skip those. Findings are appended in rule order; each
/// [`RuleViolation::rule_id`] equals the rule id.
pub fn lint(
    spec: &DynamicTopology,
    schemas: Option<&HashMap<&str, PortSchema>>,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    for rule in RULES {
        for v in (rule.check)(spec, schemas) {
            match rule.severity {
                Severity::Error => report.push(v),
                Severity::Warning | Severity::Info => report.warn(v),
            }
        }
    }
    report
}

/// Run a single rule by id.
pub fn lint_rule(
    spec: &DynamicTopology,
    schemas: Option<&HashMap<&str, PortSchema>>,
    rule_id: &str,
) -> Vec<RuleViolation> {
    RULES
        .iter()
        .find(|r| r.id == rule_id)
        .map(|r| (r.check)(spec, schemas))
        .unwrap_or_default()
}

// ── Individual checks ──────────────────────────────────────────────────────────

fn check_orphan_machine(
    spec: &DynamicTopology,
    _: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    let mut used: Vec<&str> = Vec::new();
    for link in &spec.links {
        used.push(link.out.0.as_ref());
        used.push(link.into.0.as_ref());
    }
    spec.machines
        .iter()
        .filter(|m| !used.contains(&m.name.as_ref()))
        .map(|m| {
            RuleViolation::new(
                "orphan-machine",
                format!("machines[{}]", m.name),
                "machine participates in at least one link",
                "no incoming or outgoing links",
            )
        })
        .collect()
}

fn check_unused_output_port(
    spec: &DynamicTopology,
    schemas: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    let Some(schemas) = schemas else { return Vec::new() };
    let mut consumed: Vec<(&str, &str)> = Vec::new();
    for link in &spec.links {
        consumed.push((link.out.0.as_ref(), link.out.1.as_ref()));
    }
    let mut out = Vec::new();
    for m in &spec.machines {
        let Some(schema) = schemas.get(m.name.as_ref()) else {
            continue;
        };
        for port in schema.ports() {
            if port.dir == crate::port::PortDir::Out
                && !consumed.contains(&(m.name.as_ref(), port.name))
            {
                out.push(RuleViolation::new(
                    "unused-output-port",
                    format!("{}.{}", m.name, port.name),
                    "output port is consumed by at least one link",
                    "no link reads this port",
                ));
            }
        }
    }
    out
}

fn check_no_observation(
    spec: &DynamicTopology,
    schemas: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    let Some(schemas) = schemas else { return Vec::new() };
    if spec.machines.is_empty() {
        return Vec::new();
    }
    let any_observe = schemas.values().any(|s| {
        s.ports().iter().any(|p| p.flow == FlowKind::Observe)
    });
    if any_observe {
        Vec::new()
    } else {
        vec![RuleViolation::new(
            "no-observation",
            "machines",
            "at least one Observe-flow port declared",
            "no Observe port anywhere in the topology",
        )]
    }
}

fn check_no_control(
    spec: &DynamicTopology,
    schemas: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    let Some(schemas) = schemas else { return Vec::new() };
    if spec.machines.is_empty() {
        return Vec::new();
    }
    let any_control = schemas
        .values()
        .any(|s| s.ports().iter().any(|p| p.flow == FlowKind::Control));
    if any_control {
        Vec::new()
    } else {
        vec![RuleViolation::new(
            "no-control",
            "machines",
            "at least one Control-flow port declared",
            "no Control port anywhere in the topology",
        )]
    }
}

fn check_uniform_link_kind(
    spec: &DynamicTopology,
    _: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    let mut kinds: Vec<&str> = Vec::new();
    for link in &spec.links {
        let name = match &link.kind {
            LinkKind::Inline => "Inline",
            LinkKind::BoundedBuf { .. } => "BoundedBuf",
            LinkKind::Channel { .. } => "Channel",
            LinkKind::Latest { .. } => "Latest",
            LinkKind::CasFreeRing { .. } => "CasFreeRing",
            LinkKind::SharedState => "SharedState",
        };
        if !kinds.contains(&name) {
            kinds.push(name);
        }
    }
    if kinds.len() == 1 && spec.links.len() > 1 {
        vec![RuleViolation::new(
            "uniform-link-kind",
            "links",
            "links use a mix of carriers matching traffic shape",
            format!("every link is {}", kinds[0]),
        )]
    } else {
        Vec::new()
    }
}

fn check_default_physical(
    spec: &DynamicTopology,
    _: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    if spec.machines.is_empty() {
        return Vec::new();
    }
    let all_default = spec.machines.iter().all(|m| {
        // MachinePhysicalSpec has no PartialEq; compare field-by-field.
        matches!(m.physical.execution, ExecutionHint::Async)
            && m.physical.state_heap_bytes == 4096
            && !m.physical.cache_line_align
            && !m.physical.deterministic
            && m.physical.max_cleanup_latency_us == 10_000
    });
    if all_default {
        vec![RuleViolation::new(
            "default-physical",
            "machines[].physical",
            "physical decisions made explicit (execution, budget, alignment)",
            "every MachinePhysicalSpec is default()",
        )]
    } else {
        Vec::new()
    }
}

fn check_no_funcs(
    spec: &DynamicTopology,
    _: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    if spec.funcs.is_empty() && !spec.machines.is_empty() {
        vec![RuleViolation::new(
            "no-funcs",
            "funcs",
            "pure-function layer used for stateless transforms",
            "no FuncBinding declared",
        )]
    } else {
        Vec::new()
    }
}

fn check_no_moore_headroom(
    spec: &DynamicTopology,
    _: Option<&HashMap<&str, PortSchema>>,
) -> Vec<RuleViolation> {
    if spec.machines.is_empty() {
        return Vec::new();
    }
    let any_moore = spec.machines.iter().any(|m| m.is_moore);
    // Cycle-free + no Moore: a future feedback loop would be rejected.
    let has_cycle = !crate::analysis::feedback_loops(spec).is_empty()
        || crate::analysis::inline_cycle(spec).is_some();
    if !any_moore && !has_cycle {
        vec![RuleViolation::new(
            "no-moore-headroom",
            "machines[].is_moore",
            "at least one machine declared Moore (or a cycle exists already)",
            "acyclic topology, no Moore machine",
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DynamicTopology, MachineInstance};
    use crate::link::{LinkKind, LinkSpec};
    use crate::port::PortDecl;
    use crate::resource::MachinePhysicalSpec;

    fn spec_io() -> (DynamicTopology, HashMap<&'static str, PortSchema>) {
        let spec = DynamicTopology::new()
            .with_machine(MachineInstance::new(
                "a",
                "A",
                MachinePhysicalSpec::default(),
            ))
            .with_machine(MachineInstance::new(
                "b",
                "B",
                MachinePhysicalSpec::default(),
            ))
            .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));
        // Port schemas with only Data flow.
        let a = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"));
        let b = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", a);
        schemas.insert("b", b);
        (spec, schemas)
    }

    #[test]
    fn lint_flags_missing_observation() {
        let (spec, schemas) = spec_io();
        let report = lint(&spec, Some(&schemas));
        assert!(report
            .warnings
            .iter()
            .any(|v| v.rule_id == "no-observation"));
    }

    #[test]
    fn lint_flags_default_physical_and_uniform_links() {
        let (spec, schemas) = spec_io();
        let report = lint(&spec, Some(&schemas));
        assert!(report
            .warnings
            .iter()
            .any(|v| v.rule_id == "default-physical"));
        // single link -> uniform check needs >1 links, so it must NOT fire here.
        assert!(!report
            .warnings
            .iter()
            .any(|v| v.rule_id == "uniform-link-kind"));
    }

    #[test]
    fn lint_rule_by_id() {
        let (spec, schemas) = spec_io();
        let v = lint_rule(&spec, Some(&schemas), "orphan-machine");
        // a and b are both linked, so no orphans.
        assert!(v.is_empty());
        let v = lint_rule(&spec, Some(&schemas), "no-funcs");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn lint_clean_when_axioms_satisfied() {
        // Machine with Observe port + non-default physical + func + Moore headroom.
        let spec = DynamicTopology::new()
            .with_machine(
                MachineInstance::new(
                    "a",
                    "A",
                    MachinePhysicalSpec {
                        deterministic: true,
                        ..MachinePhysicalSpec::default()
                    },
                )
                .moore(),
            )
            .with_machine(MachineInstance::new(
                "b",
                "B",
                MachinePhysicalSpec::default(),
            ))
            .with_func(crate::deploy::FuncBinding::new("f", "F"))
            .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Channel {
                capacity: 8,
                drop_when_full: false,
            }));
        let a = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::observe::<i32>("stats"));
        let b = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", a);
        schemas.insert("b", b);
        let report = lint(&spec, Some(&schemas));
        assert!(
            report.is_ok(),
            "axiom-satisfying spec should lint clean: {:?}",
            report.warnings.iter().map(|v| v.rule_id).collect::<Vec<_>>()
        );
    }
}
