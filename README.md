# BuckPkgs

BuckPkgs is a package manager for hermetic build systems, designed to become a
first-class part of a Buck2 fork rather than a general-purpose package manager.

The working thesis is:

- Nix's strongest ideas are immutable package instances, explicit inputs,
  reproducible builds, and a rich packaging corpus.
- Nix's language, lazy fixed-point package set, host-oriented installation
  model, and broad generality are not required for a package manager whose only
  job is to feed a hermetic build graph.
- Buck2 already has the execution engine BuckPkgs needs: configured graphs,
  hermetic toolchains, remote execution, action caching, and a CAS-backed
  materializer.
- The concrete jobs are narrower than "package management":
  - remove dependence on the host filesystem from builds, tests, and tasks
  - make common OSS tools such as `bash`, `awk`, `grep`, `sed`, and
    `postgresql` available as hermetic Buck2 inputs
  - make toolchains fully hermetic

BuckPkgs should therefore compile boring package data into Buck2 build work. It
should not grow into another programmable package language.

## Documents

- [DESIGN.md](./DESIGN.md): the initial technical design.
- [OPEN_QUESTIONS.md](./OPEN_QUESTIONS.md): decisions to settle before
  implementation.
- [MANIFEST_LANGUAGE.md](./MANIFEST_LANGUAGE.md): the restricted Starlark
  package-definition decision for the first prototype.
- [BOOTSTRAP.md](./BOOTSTRAP.md): the practical path from a cached foreign Buck2
  seed to a self-hosted base toolchain.
- [BOOTSTRAP_TO_BASH_SPIKE.md](./BOOTSTRAP_TO_BASH_SPIKE.md): the first concrete
  package-tree and bootstrap ladder for the implementation spike.
- [BUCK2_NATIVE_MODEL.md](./BUCK2_NATIVE_MODEL.md): how BuckPkgs should map onto
  Buck2's native graph, action, artifact, and provider model, including what can
  be prototyped before a Buck2 fork.
- [PROTOTYPE_ARCHITECTURE.md](./PROTOTYPE_ARCHITECTURE.md): how far the first
  implementation can get with ordinary Buck2 Starlark plus externally
  bootstrapped Cargo tools.
- [BUCK2_STORE_INTEGRATION.md](./BUCK2_STORE_INTEGRATION.md): the concrete Buck2
  changes needed to make `/pkgs/store/...` outputs first-class artifacts for
  targets such as `//pkgs/tools/compression/bzip2:out`.
- [BUCK2_CHANGES_PROPOSAL.md](./BUCK2_CHANGES_PROPOSAL.md): the consolidated
  Buck2 fork proposal, including first-class store outputs, symbolic identity,
  CAS interaction, store closures, and expected combined-system properties.
- [STORE_PATHS.md](./STORE_PATHS.md): the input-addressed key design for
  `/pkgs/store/<pkgs_path>`.
- [PKG_CONFIG.md](./PKG_CONFIG.md): the proposed structured model for
  role-aware native library discovery.
- [STDENV.md](./STDENV.md): what nixpkgs `stdenv` actually contains and how
  BuckPkgs should decompose it.
- [ENGINEERING.md](./ENGINEERING.md): implementation, dependency, testing, and
  performance ground rules for the project.
- [REPRODUCIBILITY.md](./REPRODUCIBILITY.md): the current determinism contract,
  known failure modes, and artifact-level verification expectations.
- [STORE_SUBSTITUTES.md](./STORE_SUBSTITUTES.md): the manifest, archive, and
  hydration model for importing finalized store objects.
- [BOOTSTRAP_ISLAND.md](./BOOTSTRAP_ISLAND.md): the graph boundary that keeps
  ordinary builds from implicitly rebuilding bootstrap turnover.

## Current Position

This repository is at the design stage. The intended first implementation is:

1. Restricted Starlark package definitions for the first prototype, lowering to
   a strict Rust data model.
2. A small package graph evaluator integrated into the Buck2 daemon.
3. Package builds lowered to normal Buck2 actions so they inherit Buck2's
   sandboxing, remote execution, caching, and materialization model.
4. A manual-first porting path from nixpkgs: keep the shape familiar enough that
   human ports are quick, without evaluating Nix expressions or recreating Nix's
   package-set machinery.

The initial milestone is intentionally narrower than Nix:

- Linux first.
- Native builds first.
- Tool binaries first, then toolchains, with library support added where Rust
  builds need native dependencies or bindgen inputs.
- A fully locked package-set composition, with everything allowed to live in one
  BuckPkgs monorepo initially.
- No general dependency solver.
- A finite set of standard builders plus one explicit escape hatch.
- No global package installation or user profile management.
- `/pkgs/store/<pkgs_path>` is the logical package ABI. Buck2 artifacts,
  providers, action caching, and CAS are the execution and realization layer
  beneath it.
- Bootstrap should optimize for reaching a useful system quickly from a foreign
  Buck2 seed, then rebuilding to eliminate foreign-seed references from final
  exported closures. BuckPkgs is not pursuing a Mes/TinyCC-style minimal-trust
  bootstrap path.
