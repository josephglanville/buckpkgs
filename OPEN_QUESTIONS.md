# BuckPkgs Implementation Questions

This document records the decisions already made, then the questions that remain
before implementation starts.

## Resolved Decisions

### Product Framing

BuckPkgs is a Buck2-native replacement for host-filesystem dependency on third-party
tools and toolchains. The first-class use cases are:

- hermetic tools for tests and tasks
- hermetic OSS binaries such as `bash`, `awk`, `grep`, `sed`, and `postgresql`
- fully hermetic toolchains

### Bootstrap Trust Root

Pinned prebuilt bootstrap archives are acceptable. Bootstrap should optimize for
getting useful quickly; BuckPkgs is not pursuing a Mes/TinyCC-style minimal
bootstrap.

### Optimization Order

Optimize first for:

1. tool binaries
2. toolchains
3. libraries where Rust/native integration needs them

After the initial tool slice, prioritize `bash`, then `gcc`.

### Package-Set Shape

Long term, package sets should be external and composable. A given monorepo
checkout must still fully lock the package-set composition it consumes.

Short term, everything may live in one BuckPkgs monorepo.

Composition uses simple ordered overlays: later package sets win.

### Compatibility Bias

Keep nixpkgs package names and output conventions unless there is a concrete
reason to diverge.

### Storage Direction

Use `/pkgs/store/<pkgs_path>` as the logical package ABI and Buck2 artifacts/CAS
as the physical realization layer.

Store paths should be pre-build-known, input-addressed, and output-specific so
ordinary Unix package recipes can embed stable absolute prefixes. Built outputs
should still be ordinary Buck2 artifacts backed by CAS tree digests.

Remote execution should receive only the required store-path closure for each
action.

## Decide Before Coding

### 1. Package Definition Format

**Decision:** use restricted Starlark for the first prototype.

**Non-negotiable constraints:**

- no Nix
- no unrestricted Starlark
- no code embedded in TOML
- no general recursion, network, or ambient filesystem reads during package
  graph construction
- the final evaluated package definition must be strict Rust-validated data

See [MANIFEST_LANGUAGE.md](./MANIFEST_LANGUAGE.md).

**Why:** if we want the first implementation to be ordinary Buck2 Starlark plus
external Rust builder binaries, restricted Starlark definitions are the smallest
path. TOML would require a separate pre-Buck generation step that turns TOML into
Starlark-visible graph data.

### Earliest Likely Fork Point

The first strong reason to fork Buck2 is probably not package identity or action
lowering. It is making `/pkgs/store/...` realization first-class instead of a
local side effect outside Buck2's tracked output model.

### 2. Resolver Model

**Question:** does BuckPkgs solve dependency versions dynamically?

**Recommendation:** no general solver in the normal path. Use a fully locked
composition of package sets with one selected version per package name and
explicit named variants only when required.

**Why:**

- consistent with hermetic repo builds
- deterministic
- avoids another large subsystem
- closer to the useful part of nixpkgs than to language-specific package
  managers

### 3. Buck2 Integration Depth

**Question:** is BuckPkgs an external companion tool or part of Buck2 itself?

**Recommendation:** long term, part of Buck2 itself, with Rust graph logic in the
daemon and package builds lowered to normal Buck2 actions.

**Why:**

- matches the project goal
- avoids duplicated caching/execution/materialization
- lets normal Buck2 targets consume package providers directly

**Pragmatic note:** because store paths can be derived entirely from BuckPkgs data,
the first implementation can likely avoid a Buck2 fork:

- BuckPkgs computes store identity itself
- package realization is represented as ordinary Buck2 graph nodes
- a BuckPkgs realization step materializes package trees into the local store

Remote execution is intentionally deferred. The design constraint for now is
only that every consumed store path has explicit closure metadata and a known
artifact backing, so a future RE implementation can mount exactly the required
absolute paths without changing package identity.

### 4. Store-Key Model

**Question:** what should name an output under `/pkgs/store`?

**Recommendation:** use an input-addressed package-output key derived from the
fully resolved package instance, output name, visible store name, and direct
dependency store paths grouped by role.

**Why:**

- the path is known before the build starts
- packages may safely embed their own absolute prefixes
- direct dependency store paths recursively commit to the transitive declared
  inputs that matter to the build
- Buck2 action digests remain execution-cache keys, and CAS output digests remain
  realized-content identities

### 5. Initial Dependency Model

**Question:** how much of nixpkgs' dependency matrix should appear in v0?

**Recommendation:** expose `tools` and `libs` in manifests for the native-only
first milestone, but keep build/host/target roles in the internal model.

**Why:**

- simple enough for the first package corpus
- does not paint us into a corner on cross-compilation
- avoids copying nixpkgs' full visible complexity before it is earned

### 6. Builders And Escape Hatches

**Question:** how expressive is a package recipe?

**Recommendation:** start with:

- `autotools`
- `cmake`
- `meson`
- `make`
- `cargo`
- `script`

The first five should cover conventions. `script` should exist but be clearly
marked as an escape hatch.

**Why:**

- captures a large share of nixpkgs' common cases
- keeps manual ports straightforward
- allows progress on difficult packages without expanding the manifest language
  into a programming language

### 7. Bootstrap Trust Root

**Question:** what are the initial trusted tools?

**Decision:** use a small foreign bootstrap prefix built by ordinary Buck2
actions, then rebuild the wider base set from source on top. Optimize for getting
useful quickly; do not pursue Mes/TinyCC-style minimal bootstrap.

### 8. First Platform Boundary

**Question:** what does milestone one support?

**Recommendation:** Linux native builds only.

**Why:**

- enough to validate the model
- aligns with the highest-value hermetic build environment
- keeps bootstrap and builder work tractable

Cross-compilation and macOS support should be designed for, not implemented
first.

### 9. Nixpkgs Relationship

**Question:** should BuckPkgs evaluate nixpkgs or port from it?

**Recommendation:** port manually from nixpkgs. Keep BuckPkgs close enough in shape
that the translation is quick, but do not plan around a general importer and
never make Nix evaluation part of the BuckPkgs build path.

**Why:**

- preserves the low-analysis-time goal
- avoids inheriting Nix semantics through the back door
- still lets us exploit nixpkgs' package knowledge
- avoids spending effort on a partial Nix evaluator disguised as tooling

### 10. First End-To-End Slice

**Question:** what proves the design?

**Recommendation:** a tool-first slice:

- `gnused`
- `gnugrep`
- `gawk`
- one package or ordinary Buck2 action that consumes them as hermetic tools
- one ordinary Buck2 target depending on a BuckPkgs provider

This slice forces source fetching, builders, multiple outputs, dependency
handling, and rule consumption while staying aligned with the first product
goal.

## Can Wait Until After The First Slice

### Binary Distribution Outside Buck2

Remote execution and the Buck2 action cache are enough at first. A standalone
BuckPkgs binary cache protocol can wait until there is evidence it is needed.

### Full Cross-Compilation Surface

Keep enough type information internally to support it later. Do not expose the
full nixpkgs-style matrix until compiler packages force the issue.

### Garbage Collection

If outputs are views over Buck2 materialization, BuckPkgs-specific GC is not an
early blocker.

### Registry UX

The initial package set can live in-tree. External composable package sets are a
later architectural requirement, but a public registry, search index, and
publishing workflow are later concerns.

### Arbitrary Overrides

Projects can start with:

- package-set revision pins
- explicit local forks
- explicit variant packages

There is no need to recreate nixpkgs overlay power before we know which override
patterns matter in Buck2 repos.

## Questions To Answer Together

1. Which real packages, if any, force a `/pkgs/store` compatibility layer rather
   than artifact-relative layouts, wrappers, or provider metadata?
2. Which exact foreign-seed contents should BuckPkgs build for the first practical
   bootstrap?
3. How aggressively should the first self-hosted milestone require seed-free
   closures: only final `gcc`/`binutils`/`libc`, or the full base tool set?
4. Which parts of nixpkgs' ordinary bootstrap path are essential to preserve,
   and which are artifacts of Nix's own store/wrapper machinery?

## Suggested First Decisions

If no stronger constraint appears, I would proceed with:

1. Use restricted Starlark for the first prototype.
2. Linux/native-only milestone one.
3. One locked package-set composition, no solver.
4. Buck2-native graph integration.
5. `tools` plus `libs` as the visible dependency surface.
6. Buck2 CAS as the storage layer.
7. Manual-first nixpkgs ports, with only small assistive tools that do not
   require evaluating Nix.
8. A small explicit foreign bootstrap prefix built by ordinary Buck2 actions.
