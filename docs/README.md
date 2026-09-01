> **Language:** English · [中文](zh-cn/README.md)

# axiom Documentation

Documentation for axiom is maintained in two languages (English is the authoritative default):

| Language | Index | Docs |
|---|---|---|
| **English** (default) | [`docs/en-us/README.md`](en-us/README.md) | `vision` · `foundations` · `core` · `semantics` · `unified` |
| **中文** | [`docs/zh-cn/README.md`](zh-cn/README.md) | `vision` · `foundations` · `core` · `semantics` · `unified` |

Choose a language above to read the documentation:
vision (the charter: why axiom, the four target systems, the AI-era consistency-machine
thesis — no new axioms; the formal volumes prevail), then the formal specification:
foundations (definitions · axioms · theorems), core (the compile-time core `cell_core`),
semantics (the physical-layer Carrier), and unified (the unified model: substitution,
definition–activation, schemas — axiom's upgraded design beyond the static blueprint).

> **Theory corpus (non-normative):** derivation archives, meta-theory
> ([boundary-ontology](internal/theory/boundary-ontology.md) ·
> [meta-foundations](internal/theory/meta-foundations.md)), unrealized-directions registry
> ([frontier-notes](internal/theory/frontier-notes.md)), and the historical archive
> ([theory-archive](internal/theory/theory-archive.md)) live under
> [`docs/internal/theory/`](internal/theory/README.md). On conflicts, the formal specification prevails.

> **Workspace layout (non-normative):** the workspace is layered — `core/` (compile-time
> core), `semantics/` (physical layer; source under checks/movers/seams/drive), `instances/`
> (ready-made swap-ins; feature doors closed by default; tokio async backend), `examples/`
> (cross-layer comprehensive use cases, e.g. SQL-over-Redis). The async path, observation,
> and the postponed third-party adapters are covered in the semantics volume appendix.

> **Maintenance rule:** English is the authoritative default. When a spec changes, update the
> English document first, then mirror the change into the Chinese document. Each English document
> links to its Chinese counterpart and vice versa, using relative paths (never absolute).

## Version and Stability Policy (A5)

- **`cell_core` (the compile-time core) = stable**: semantic changes are constitution-level
  decisions (closed-boundary checklist, §8.3); semantic regressions are rejected. New
  combinators may be added only as instances of concepts 1–5 (collective ruling if in doubt).
- **semantics modules carry per-module `Stability` markers**: `stable` (carrier basics, flow
  drivers), `experimental` (obligation/contract/system under active constitution work).
  A `stable` marker is the only bettability promise before 1.0: third parties may pin against
  it (subject to the versioning rule below); `experimental` modules carry no bettability
  promise and may change in any minor release.
- **Versioning**: before 1.0, breaking changes bump the minor version (SemVer); each
  breaking change ships a concept-migration note (which names moved, which semantics
  shifted, where the concept lives now) even without a compatibility layer.
- **Contract evolution, three modes** (frozen-face discipline): a frozen contract changes
  only through one of — (1) *snapshot coexistence* (the old contract remains importable
  under its historical name; retirement keeps the name), (2) *feature gate* (the new
  semantics rides a feature; the default face is unchanged), or (3) *breaking migration*
  (SemVer minor bump before 1.0 + concept-migration note). Anything outside the three
  modes is not a permitted change to a frozen contract.
- **`forbid(unsafe_code)` persists**; if unsafe ever becomes necessary, it is isolated into a
  dedicated feature with a documented obligation proof (`docs`, modality ④ exhibition), never
  into the stable core.
