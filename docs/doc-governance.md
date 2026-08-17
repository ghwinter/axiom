# Documentation Standards

This document defines how axiom documentation is organized, written, and
maintained. It is the single source of truth for documentation practice:
every fact has exactly one home, and every document follows one of the styles
defined below.

## 1. Document tiers

axiom separates documents into a public tier taxonomy and an internal
workspace. Each tier has a distinct audience, language, and style. A fact
belongs to exactly one tier; other tiers link to it and never restate it
(one home per fact).

### Public tiers

| Tier | Audience | Language | Style model | Location |
|------|----------|----------|-------------|----------|
| L1 API reference | Users of a crate | English | Rust standard library / rustdoc conventions | in-source `///` docs |
| L2 Narrative | New and existing users | English | Framework user guides (intro, architecture, concepts) | `README.md`, `docs/*.md` |
| L3 Formal | Mathematically inclined readers | English | Academic paper (definitions, axioms, theorems, proofs) | `docs/foundations.md`, `docs/structural-model.md` |
| L4 Release | Maintainers and integrators | English | Keep a Changelog conventions | `CHANGELOG.md`, `docs/migration-*.md` |

- **L1** documents every public item of a crate. Follow the standard-library
  conventions: `///` doc comments on items, `//!` module docs with an example
  as the first thing readers see. Code examples must compile as doctests.
- **L2** explains the system to a reader who has not seen it before. It may be
  narrative (an introduction or tutorial) or architectural (component
  overview, execution model, design rationale). It links to L1 for precise
  signatures and to L3 for formal statements.
- **L3** states definitions, axioms, theorems, corollaries, and proofs with a
  consistent numbering scheme. It is written for precision: every symbol is
  defined before use, and every claim is either proved or explicitly marked as
  an assumption.
- **L4** records what changed between releases. Follow Keep a Changelog
  (`Added` / `Changed` / `Removed` / `Fixed` sections under a version heading
  with a date). Migration guides pair each breaking change with its
  replacement.

### Internal workspace

| Tier | Audience | Language | Content | Location |
|------|----------|----------|---------|----------|
| I1 Design records | Project maintainers | Chinese (working language) | Analysis, design rationale, iteration notes, comparisons, roadmaps | `docs/internal/*.md` |

- **I1** is a working space, not publication. It records why design decisions
  were made, including discarded alternatives and empirical data that shaped
  the decision. It is allowed to be informal and time-stamped.
- I1 documents are never linked from public tiers; public tiers restate only
  the settled conclusion, with its justification, in L2 or L3 form.
- The internal workspace never replaces a public document: once a design is
  settled, its conclusion lives in the public tiers and the I1 record is
  archived.

## 2. Language policy

- Public tiers (L1–L4) are written in English. This is the language of precise
  technical description; identifiers are never translated.
- I1 design records are written in Chinese, the working language of the
  maintainers. Code identifiers remain in English.
- A public document contains no narrative history ("previously", "now",
  "changed from"), no work-in-progress markers, and no internal plan labels
  (milestones, task codes, phase numbers). It states the system as it is.

## 3. Writing rules

- **Current state**: describe the system as it exists, not how it changed.
  Change stories belong in L4 (changelog, migration guide) and I1 (design
  records).
- **One home per fact**: before writing anything, decide which tier owns the
  fact. If more than one tier needs it, exactly one elaborates; the others
  link to it.
- **Link over restate**: cross-tier references use relative Markdown links,
  not bare file names.
- **Identifiers verbatim**: types, traits, functions, and paths appear exactly
  as in the code, never translated or paraphrased.
- **Precision over metaphor**: use a term only in its literal sense. "Contract"
  means an obligation or invariant; "boundary" means a literal process,
  safety, or transaction boundary. Do not inflate emphasis; bold only the
  clause that changes behavior.

## 4. Slop checklist

Reread each document for these failure modes before committing:

- **Narrative history**: "previously / now / has changed" — write the current
  fact; the change story goes to the changelog and the design record.
- **Status labels**: "implemented!", "future: …" — status decays. The
  repository is the authority; documents describe present reality.
- **Hand-copied catalogs**: any table or matrix that can be generated from the
  source (module lists, evidence registers) is not maintained by hand.
- **Reasoning transcripts**: do not reproduce long derivations of "why". Keep
  the conclusion and a one-sentence justification; the full reasoning goes to
  the design record.
- **Paragraph walls**: one paragraph per rule; split compound sentences.
- **Emphasis inflation**: all-bold text is no emphasis at all.
- **Metaphor creep**: replace figurative language with exact terms.

## 5. Word budgets

Budgets keep each tier focused. Exceeding one is a signal that content belongs
to another tier or to an I1 record.

| Document | Budget (words) |
|----------|----------------|
| `README.md` | 2500 |
| `docs/philosophy.md` | 5000 |
| `docs/foundations.md` | 4000 |
| `docs/architecture.md` | 6500 |
| `docs/design-principles.md` | 1500 |
| `docs/adapters.md` | 1500 |
| `docs/architecture_diagrams.md` | 2000 |
| `docs/structural-model.md` | 4000 |
| `docs/zero-cost-paradigm.md` | 2500 |
| `docs/migration-*.md` | 2000 each |
| `docs/doc-governance.md` (this document) | 1500 |

Remediation order when a document exceeds its budget: migrate content that
belongs to another tier, leaving one linking line; then compress the remaining
prose; only raise the budget for content that is genuinely necessary, and
record the reason.

## 6. Decision records

axiom is a philosophy-driven project: the "why" of a decision matters as much
as the code, and the reasoning must be reviewable in the repository.

A non-trivial change (a new contract, an execution-model change, a breaking
API change) must be accompanied by a decision record in `docs/internal/` (I1
style). The record states:

- the motivation (the meta-problem, constraint, or evidence),
- the decision (one or two sentences),
- the rationale (why this choice),
- the cost (what was given up, the boundary),
- the verification (how the decision is validated: tests, benchmarks, proofs).

Settled decisions are summarized in the public tiers with their justification;
the full record remains in the internal workspace.
