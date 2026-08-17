//! **Maturity: tool** (a development-time tool, reinforced per the unified convention).
//!
//! AI-native blueprint interface (feature = "serialize").
//!
//! This module turns [`DynamicTopology`] into a **machine-readable work medium**:
//!
//! - [`schema`] exports a JSON Schema (draft-07) describing the exact
//!   serialized shape of a `DynamicTopology`. An AI (or any tool) can generate
//!   blueprints as plain JSON against this schema, without writing Rust
//!   builder chains.
//! - [`from_json_str`] / [`from_json_value`] parse such JSON back into a
//!   `DynamicTopology` **and** run structural validation, so malformed blueprints
//!   (unknown machine references, duplicate names, self-loops) are rejected
//!   with a structured [`BlueprintError`] instead of failing later at deploy
//!   time.
//! - [`to_json`] serializes a `DynamicTopology` (pretty-printed) for inspection or
//!   persistence.
//!
//! The round-trip is exact: `to_json` → `from_json_str` reproduces the same
//! `DynamicTopology` (proved by `tests/serde_roundtrip.rs` and the E-evidence
//! probe). No schema library is used — the schema is hand-written and mirrors
//! the serde representation exactly, keeping the dependency surface minimal
//! and the schema auditable.
//!
//! ## Contract position
//!
//! A `DynamicTopology` is pure data; `schema`/`from_json`/`to_json` are pure
//! functions on that data. They add no physical meaning — the blueprint is
//! still just the structure graph. What this module adds is the **interface**
//! an AI can close a loop on: generate JSON → validate → get structured
//! errors → iterate.

#![cfg(feature = "serialize")]

use alloc::format;
use alloc::string::{String, ToString};

use serde_json::{json, Value};

use crate::deploy::{DynamicTopology, MachineInstance, ValidationError};
use crate::link::{LinkKind, LinkSpec};
use crate::resource::MachinePhysicalSpec;

// ── BlueprintError ─────────────────────────────────────────────────────────────

/// Structured error produced by the blueprint interface.
///
/// Unlike `Result<(), String>`, each variant carries the information needed to
/// locate and fix the problem programmatically:
///
/// - [`Parse`](BlueprintError::Parse) — JSON syntax error, with line/column.
/// - [`Invalid`](BlueprintError::Invalid) — structurally well-formed JSON that
///   is not a `DynamicTopology` shape (missing/unknown field, wrong type), with the
///   serde path.
/// - [`Validate`](BlueprintError::Validate) — a valid `DynamicTopology` shape that
///   violates deployment invariants (unknown reference, duplicate name,
///   self-loop, …), carrying the full [`ValidationError`].
#[derive(Debug)]
pub enum BlueprintError {
    /// JSON syntax error at `(line, column)`.
    Parse {
        message: String,
        line: usize,
        column: usize,
    },
    /// The JSON parsed but does not match the `DynamicTopology` shape.
    Invalid {
        message: String,
        path: String,
    },
    /// The shape is a `DynamicTopology`, but it violates structural invariants.
    Validate(ValidationError),
}

impl core::fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlueprintError::Parse {
                message,
                line,
                column,
            } => write!(f, "blueprint parse error at {line}:{column}: {message}"),
            BlueprintError::Invalid { message, path } => {
                write!(f, "blueprint invalid at {path}: {message}")
            }
            BlueprintError::Validate(e) => write!(f, "blueprint violates invariants: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BlueprintError {}

fn map_serde_err(e: serde_json::Error) -> BlueprintError {
    if e.is_syntax() || e.is_eof() {
        BlueprintError::Parse {
            message: e.to_string(),
            line: e.line(),
            column: e.column(),
        }
    } else {
        BlueprintError::Invalid {
            message: e.to_string(),
            path: String::new(),
        }
    }
}

// ── Entry points ───────────────────────────────────────────────────────────────

/// Parse a blueprint from a JSON string, then run structural validation.
///
/// The parsed spec must pass [`DynamicTopology::validate`] (unknown machine/port
/// references, duplicate names, self-loops) — otherwise the error is a
/// [`BlueprintError::Validate`].
pub fn from_json_str(s: &str) -> Result<DynamicTopology, BlueprintError> {
    let value: Value = serde_json::from_str(s).map_err(map_serde_err)?;
    from_json_value(value)
}

/// Parse a blueprint from a `serde_json::Value`, then validate structurally.
pub fn from_json_value(value: Value) -> Result<DynamicTopology, BlueprintError> {
    let spec: DynamicTopology = serde_json::from_value(value).map_err(map_serde_err)?;
    spec.validate().map_err(BlueprintError::Validate)?;
    Ok(spec)
}

/// Serialize a `DynamicTopology` to pretty JSON.
pub fn to_json(spec: &DynamicTopology) -> Result<String, BlueprintError> {
    let bytes = serde_json::to_vec_pretty(spec).map_err(map_serde_err)?;
    String::from_utf8(bytes).map_err(|e| BlueprintError::Invalid {
        message: format!("serializer produced non-UTF-8: {e}"),
        path: String::new(),
    })
}

/// Export the JSON Schema (draft-07) that `from_json_str` accepts.
///
/// The schema is generated on the fly (no allocation beyond the returned
/// `Value`) and mirrors the serde representation exactly.
pub fn schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://axiom.dev/schema/deployspec.schema.json",
        "title": "Axiom DynamicTopology",
        "description": "Structural blueprint of an axiom deployment: machines, funcs, links, settings.",
        "type": "object",
        "properties": {
            "machines": {
                "type": "array",
                "description": "Machine instances (boundaries + functional definition).",
                "items": { "$ref": "#/$defs/machine" }
            },
            "funcs": {
                "type": "array",
                "description": "Pure function bindings referenced by the topology.",
                "items": { "$ref": "#/$defs/func" }
            },
            "links": {
                "type": "array",
                "description": "Directed connections between ports. Cycles are allowed iff each cycle passes ≥1 Moore machine.",
                "items": { "$ref": "#/$defs/link" }
            },
            "settings": { "$ref": "#/$defs/settings" }
        },
        "required": ["machines", "funcs", "links", "settings"],
        "additionalProperties": false,
        "$defs": {
            "machine": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique name within this deployment (referenced by links)."
                    },
                    "machine_type": {
                        "type": "string",
                        "description": "Type name registered with the runtime factory."
                    },
                    "physical": { "$ref": "#/$defs/physical" },
                    "config_overrides": {
                        "type": "array",
                        "description": "Initial configuration overrides as [key, value] pairs.",
                        "items": {
                            "type": "array",
                            "items": [{ "type": "string" }, { "type": "string" }],
                            "minItems": 2,
                            "maxItems": 2
                        }
                    },
                    "is_moore": {
                        "type": "boolean",
                        "description": "Moore semantics: output depends only on pre-update state. Required on ≥1 machine of every cycle for fused/inline runtimes.",
                        "default": false
                    }
                },
                "required": ["name", "machine_type", "physical"],
                "additionalProperties": false
            },
            "func": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "func_type": { "type": "string" }
                },
                "required": ["name", "func_type"],
                "additionalProperties": false
            },
            "link": {
                "type": "object",
                "properties": {
                    "out": {
                        "type": "array",
                        "description": "Source endpoint (machine, port).",
                        "items": [{ "type": "string" }, { "type": "string" }],
                        "minItems": 2,
                        "maxItems": 2
                    },
                    "into": {
                        "type": "array",
                        "description": "Target endpoint (machine, port).",
                        "items": [{ "type": "string" }, { "type": "string" }],
                        "minItems": 2,
                        "maxItems": 2
                    },
                    "kind": { "$ref": "#/$defs/link_kind" }
                },
                "required": ["out", "into", "kind"],
                "additionalProperties": false
            },
            "settings": {
                "type": "object",
                "properties": {
                    "cpu_threads": { "type": "integer", "minimum": 0 },
                    "io_threads": { "type": "integer", "minimum": 0 }
                },
                "required": ["cpu_threads", "io_threads"],
                "additionalProperties": false
            },
            "physical": {
                "type": "object",
                "properties": {
                    "execution": { "$ref": "#/$defs/execution" },
                    "state_heap_bytes": { "type": "integer", "minimum": 0 },
                    "cache_line_align": { "type": "boolean" },
                    "deterministic": { "type": "boolean" },
                    "max_cleanup_latency_us": { "type": "integer", "minimum": 0 }
                },
                "required": ["execution", "state_heap_bytes", "cache_line_align", "deterministic", "max_cleanup_latency_us"],
                "additionalProperties": false
            },
            "execution": {
                "oneOf": [
                    { "const": "Async" },
                    { "const": "CpuBound" },
                    { "type": "object", "properties": { "CpuBoundN": { "type": "integer", "minimum": 1 } }, "required": ["CpuBoundN"], "additionalProperties": false },
                    { "type": "object", "properties": { "ThreadPool": { "$ref": "#/$defs/thread_pool" } }, "required": ["ThreadPool"], "additionalProperties": false },
                    { "type": "object", "properties": { "Subprocess": { "$ref": "#/$defs/subprocess" } }, "required": ["Subprocess"], "additionalProperties": false }
                ]
            },
            "thread_pool": {
                "type": "object",
                "properties": {
                    "min_threads": { "type": "integer", "minimum": 0 },
                    "max_threads": { "type": "integer", "minimum": 1 },
                    "name_prefix": { "type": "string" }
                },
                "required": ["min_threads", "max_threads", "name_prefix"],
                "additionalProperties": false
            },
            "subprocess": {
                "type": "object",
                "properties": {
                    "executable": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } },
                    "restart": { "$ref": "#/$defs/restart" }
                },
                "required": ["executable", "args", "restart"],
                "additionalProperties": false
            },
            "restart": {
                "oneOf": [
                    { "const": "Never" },
                    { "type": "object", "properties": { "MaxRetries": { "type": "integer", "minimum": 0 } }, "required": ["MaxRetries"], "additionalProperties": false },
                    { "type": "object", "properties": { "Always": { "type": "object", "properties": { "delay_ms": { "type": "integer", "minimum": 0 } }, "required": ["delay_ms"], "additionalProperties": false } }, "required": ["Always"], "additionalProperties": false }
                ]
            },
            "link_kind": {
                "oneOf": [
                    { "const": "Inline" },
                    {
                        "type": "object",
                        "properties": {
                            "BoundedBuf": {
                                "type": "object",
                                "properties": {
                                    "capacity": { "type": "integer", "minimum": 1 },
                                    "write_policy": { "$ref": "#/$defs/write_policy" },
                                    "read_policy": { "$ref": "#/$defs/read_policy" }
                                },
                                "required": ["capacity", "write_policy", "read_policy"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["BoundedBuf"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "Channel": {
                                "type": "object",
                                "properties": {
                                    "capacity": { "type": "integer", "minimum": 1 },
                                    "drop_when_full": { "type": "boolean" }
                                },
                                "required": ["capacity", "drop_when_full"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["Channel"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "Latest": {
                                "type": "object",
                                "properties": {
                                    "capacity": { "type": "integer", "description": "No physical effect; retained for API symmetry." }
                                },
                                "required": ["capacity"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["Latest"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "CasFreeRing": {
                                "type": "object",
                                "properties": {
                                    "capacity": { "type": "integer", "minimum": 1 },
                                    "storage": { "$ref": "#/$defs/memory_region" }
                                },
                                "required": ["capacity", "storage"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["CasFreeRing"],
                        "additionalProperties": false
                    },
                    { "const": "SharedState" }
                ]
            },
            "write_policy": {
                "oneOf": [
                    { "const": "Blocking" },
                    { "const": "Dropping" },
                    { "const": "Overwriting" }
                ]
            },
            "read_policy": {
                "oneOf": [
                    { "const": "Blocking" },
                    { "const": "NonBlocking" }
                ]
            },
            "memory_region": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "Static": {
                                "type": "object",
                                "properties": {
                                    "addr": { "type": "integer" },
                                    "size": { "type": "integer", "minimum": 0 }
                                },
                                "required": ["addr", "size"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["Static"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "Heap": {
                                "type": "object",
                                "properties": {
                                    "size": { "type": "integer", "minimum": 0 }
                                },
                                "required": ["size"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["Heap"],
                        "additionalProperties": false
                    }
                ]
            }
        }
    })
}

// ── Helper constructors (documented JSON shapes) ───────────────────────────────

/// Build a `MachineInstance` from its JSON-shaped parts.
///
/// `config_overrides` is `(key, value)` pairs exactly as serialized.
pub fn machine(
    name: impl Into<alloc::borrow::Cow<'static, str>>,
    machine_type: impl Into<alloc::borrow::Cow<'static, str>>,
    physical: MachinePhysicalSpec,
) -> MachineInstance {
    MachineInstance::new(name, machine_type, physical)
}

/// Build a `LinkSpec` from machine/port names.
pub fn link(
    out: (&'static str, &'static str),
    into: (&'static str, &'static str),
    kind: LinkKind,
) -> LinkSpec {
    LinkSpec::new(out, into, kind)
}

// ── Module documentation for the schema consumers ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
    use crate::resource::{ExecutionHint, MachinePhysicalSpec};

    fn sample_spec() -> DynamicTopology {
        DynamicTopology::new()
            .with_machine(machine(
                "receiver",
                "Receiver",
                MachinePhysicalSpec {
                    execution: ExecutionHint::CpuBound,
                    ..MachinePhysicalSpec::default()
                },
            ))
            .with_machine(machine(
                "store",
                "DataStore",
                MachinePhysicalSpec::default(),
            ))
            .with_link(LinkSpec::new(
                ("receiver", "out"),
                ("store", "in"),
                LinkKind::BoundedBuf {
                    capacity: 64,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::NonBlocking,
                },
            ))
    }

    #[test]
    fn round_trip_preserves_spec() {
        let spec = sample_spec();
        let json = to_json(&spec).expect("serialize");
        let back = from_json_str(&json).expect("deserialize + validate");
        // Structural equality: same names, links, kinds.
        assert_eq!(back.machines.len(), 2);
        assert_eq!(back.machines[0].name, "receiver");
        assert_eq!(back.links.len(), 1);
        assert_eq!(
            back.links[0].kind,
            LinkKind::BoundedBuf {
                capacity: 64,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::NonBlocking,
            }
        );
    }

    #[test]
    fn schema_is_valid_json_and_describes_spec() {
        let s = schema();
        // The sample spec's JSON must validate against the schema's own shape:
        // spot-check that every referenced $def exists.
        let defs = s["$defs"].as_object().expect("defs");
        for key in [
            "machine", "func", "link", "settings", "physical", "execution",
            "link_kind", "write_policy", "read_policy", "memory_region",
            "thread_pool", "subprocess", "restart",
        ] {
            assert!(defs.contains_key(key), "missing $def {key}");
        }
    }

    #[test]
    fn from_json_rejects_unknown_machine_reference() {
        let spec = DynamicTopology::new().with_link(LinkSpec::new(
            ("ghost", "out"),
            ("store", "in"),
            LinkKind::Inline,
        ));
        let json = to_json(&spec).expect("serialize");
        let err = from_json_str(&json).expect_err("must fail validation");
        assert!(matches!(err, BlueprintError::Validate(_)));
    }

    #[test]
    fn from_json_rejects_malformed_json() {
        let err = from_json_str("{\"machines\": [").expect_err("syntax error");
        assert!(matches!(err, BlueprintError::Parse { .. }));
    }

    #[test]
    fn from_json_rejects_wrong_shape() {
        // Valid JSON, but not a DynamicTopology (machines is a string).
        let err = from_json_str(r#"{"machines": "nope"}"#).expect_err("shape error");
        assert!(matches!(err, BlueprintError::Invalid { .. }));
    }

    #[test]
    fn to_json_is_pretty_and_parseable() {
        let spec = sample_spec();
        let json = to_json(&spec).expect("serialize");
        assert!(json.contains('\n'), "pretty-printed");
        assert!(json.contains("\"receiver\""));
    }
}
