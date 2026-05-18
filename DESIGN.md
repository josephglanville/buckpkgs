# BuckPkgs Design

## 1. Purpose

BuckPkgs is a package manager for exactly one environment: hermetic builds managed
by Buck2.

It exists to:

1. remove dependence on the host filesystem from builds, tests, and tasks
2. provide hermetic OSS tools on demand, such as shells, text utilities, and
   services needed by tests
3. provide fully hermetic toolchains

A BuckPkgs package is not something installed into the host system. It is an
immutable build input or output that can participate directly in Buck2's graph.

## 2. Goals

BuckPkgs should:

1. Make third-party packages as reproducible and cacheable as first-party build
   targets.
2. Preserve the valuable parts of the Nix model:
   - explicit inputs
   - fixed-output source fetching
   - immutable package instances
   - multiple outputs
   - package-set-scale reuse
3. Reuse Buck2 wherever Buck2 is already strong:
   - configured graphs
   - hermetic toolchains
   - action execution
   - remote execution
   - CAS and materialization
4. Keep package evaluation bounded, predictable, and fast.
5. Make nixpkgs a practical migration source without importing Nix as a runtime
   dependency.
6. Optimize first for executable tools, then toolchains, with enough library
   support for Rust-native integration points such as bindgen inputs and linked
   C/C++ dependencies.

## 3. Non-goals

BuckPkgs is not trying to be:

- a general host package manager
- a replacement for apt, Homebrew, NixOS, or user profiles
- a new language runtime
- a general-purpose dependency solver
- a compatibility layer for arbitrary nixpkgs expressions
- a distribution mechanism for untrusted arbitrary packages

The absence of those features is part of the design, not an omission to paper
over later.

## 4. What To Keep From Nix

Keep:

- **Derivation-like identity**: a package instance is determined by its inputs,
  configuration, and build recipe.
- **Fixed-output fetches**: network inputs are accepted only with expected
  content digests.
- **Immutable outputs**: consumers depend on package instances, never on mutable
  install locations.
- **Multiple outputs**: `out`, `dev`, `bin`, `doc`, and similar splits are often
  useful and should survive.
- **Packaging knowledge**: nixpkgs contains years of hard-won recipes and build
  conventions worth porting.
- **Names and output conventions**: keep nixpkgs naming and output splits unless
  there is a concrete reason to diverge.

Do not keep:

- the Nix language
- lazy package-set evaluation
- arbitrary overlays and recursive override chains
- implicit fixed points such as `callPackage`
- the global `/nix/store` as the center of the UX
- host installation semantics

The important idea is derivations, not Nix expressions.

## 5. Design Principles

### 5.1 Definitions Must Lower To Data

The authoring syntax is not fixed yet. It may be strict data such as TOML, or a
restricted Starlark DSL if the spike shows that a pure data format becomes
contorted for real packages.

Either way, the evaluated result must be strict data, validated by Rust:

- no arbitrary evaluation
- no general recursion
- no ambient filesystem reads
- no network access during package graph construction

If Starlark is chosen, it should be an authoring convenience that returns a
schema value, not an open-ended way to construct arbitrary build graphs. If TOML
is chosen, inline shell or code-like expressions are not acceptable; hooks must
live in separate files referenced by the manifest.

The definition surface should allow only finite selectors over known build
properties such as OS, CPU, and build mode.

### 5.2 Build-System-Native

BuckPkgs should not run beside Buck2 as a separate package universe. Package
instances should become Buck2 graph nodes and Buck2 actions. The same daemon,
configuration model, execution scheduler, remote cache, and materializer should
apply to both first-party and third-party work.

At the same time, BuckPkgs should keep stable absolute `/pkgs/store/...` prefixes as
the package ABI. Requiring the whole third-party package corpus to become fully
relocatable would fight both conventional Unix builds and the nixpkgs recipe
base BuckPkgs is meant to learn from.

### 5.3 Locked Package Sets, No Solver

The normal mode should be a fully locked composition of package sets with one
selected version per package name and explicit variants when necessary. Projects
consume a known package universe, closer to nixpkgs than to Cargo or npm.

The short-term development layout may keep everything in one BuckPkgs monorepo.
The long-term design should allow external package sets to be composed, while a
given checkout still pins the exact package-set revisions and source digests it
uses.

That means:

- dependency references are resolved by package-set lookup
- there is no SAT solve during a normal build
- lockfiles pin package-set revisions and source digests, not solver results

### 5.4 Explicit Escape Hatches

Most packages should use typed builders such as `autotools`, `cmake`, `meson`,
`make`, or `cargo`. Some packages need shell. Shell should exist, but be
visibly exceptional and fully declared.

### 5.5 Native-First, Cross-Ready

The first milestone should target native Linux builds. The in-memory model must
still record build, host, and target platforms so cross-compilation can be added
without redesigning package identity later.

### 5.6 Tool-First

The first useful corpus should optimize for executable tools:

- shells such as `bash`
- text tools such as `awk`, `grep`, and `sed`
- services used by tests such as `postgresql`

Toolchains are the next priority. Libraries matter, especially for a large Rust
monorepo with native dependencies or bindgen inputs, but they are not the first
optimization target.

### 5.7 Practical Bootstrap

Bootstrap should optimize for time-to-useful-system, not for the smallest
possible trust root.

A foreign seed built by ordinary Buck2 actions is acceptable. BuckPkgs should then
rebuild the base system in explicit stages until the final base tools and
toolchain no longer reference the foreign seed or foreign toolchain outputs. The
project is not trying to reproduce nixpkgs' `minimal-bootstrap` chain based on
Mes and TinyCC.

Claims about produced binaries are part of the design contract, not guesses.
When BuckPkgs says an output uses a particular interpreter, RPATH, dependency set, or
contains no foreign references, the implementation should verify that claim from
the produced artifact itself with tools such as `readelf`, reference scanning,
and other format-aware inspection as appropriate.

## 6. High-Level Architecture

```text
package definitions
      |
      v
Rust schema parser + package-set index
      |
      v
configured BuckPkgs package graph
      |
      v
Buck2 action lowering
      |
      v
Buck2 execution / RE / CAS / materializer
      |
      v
BuckPkgs providers consumed by Buck2 targets and toolchains
```

### 6.1 Package Sets And Locks

During early development, the package set may be a directory tree plus a lock
file:

```text
pkgs/
  packages/
    gnu/hello/package.pkgs.toml
    compression/zlib/package.pkgs.toml
  package-set.toml
  package-set.lock
```

Long term, a workspace should be able to compose multiple package sets, for
example an upstream base set plus one or more organization-specific sets. The
composition rule is intentionally simple: ordered overlays, with the most recent
set winning when multiple locked sets define the same package name. The
workspace lock must name the exact revisions and content digests of every set in
the composition.

The daemon parses the locked composition into a compact index keyed by canonical
package IDs. The index should be cheap to load, cacheable, and incrementally
invalidated when a package definition changes.

### 6.2 Package Graph

A **package definition** is what is written in the chosen definition format.

A **package instance** is a definition after applying:

- platform configuration
- selected outputs
- source digests
- resolved dependency instances
- builder version
- relevant environment inputs

The package instance is the unit of reproducibility and caching.

### 6.3 Action Lowering

Each package instance lowers to ordinary Buck2 actions:

1. fetch fixed-output sources
2. unpack
3. patch
4. configure
5. build
6. test, when enabled
7. install into declared output directories
8. fix up outputs

The important constraint is that BuckPkgs should not invent a second executor. It
should produce work that Buck2 already knows how to schedule and cache.

For the initial implementation, each install output should receive a
pre-build-known `/pkgs/store/...` path derived from a BuckPkgs store-path key. The
actual bytes for that path are still produced as ordinary Buck2 artifacts and
backed by CAS tree digests.

Until Buck2 understands BuckPkgs store outputs directly, the prototype has to fake
that shape: builders configure and install against the final logical
`/pkgs/store/...` prefix, stage the install through `DESTDIR`, then hand Buck2 a
copied artifact tree. That is an implementation hack, not the intended model.
Once BuckPkgs lives inside a Buck2 fork, Buck2 should learn the package store as a
first-class output namespace so package artifacts can be produced at their
logical store paths without this staging/copy indirection.

### 6.4 Providers

The minimum provider set should include:

- `PkgsPackageInfo`: store paths by output, declared outputs, dependency closure,
  metadata
- `DefaultInfo`: default artifacts
- `RunInfo`: executable outputs where appropriate
- a Unix-env-style BuckPkgs provider for transitive environment fragments

Adapters can later expose packages as:

- C/C++ toolchain pieces
- compiler tools
- Python interpreters
- `pkg-config` metadata
- shell environments for actions

When an action needs a composed tool or library environment, BuckPkgs should build a
normal Buck2 artifact tree for that environment and mount only the required
store-path closure into the action.

### 6.5 Why Not External Cells

Buck2 external cells are useful for obtaining source trees. They are not a good
primary abstraction for BuckPkgs packages because they model only source material,
not:

- package build steps
- dependency classes
- output splits
- derivation identity
- runtime closures

BuckPkgs may reuse some ideas from external cells or add a BuckPkgs-backed origin later,
but packages themselves should be first-class package nodes, not disguised
cells.

## 7. Package Model

### 7.1 Definition Surface

The authoring format is intentionally not settled yet. Two candidates deserve a
real spike:

- strict TOML plus file references for non-declarative hooks
- restricted Starlark that evaluates to a fixed Rust schema

The following is a schema sketch, not a final syntax commitment:

```toml
[package]
name = "hello"
version = "2.12.3"
description = "GNU Hello"
license = "GPL-3.0-or-later"

[source]
kind = "url"
urls = ["mirror://gnu/hello/hello-2.12.3.tar.gz"]
sha256 = "sha256-DV9gFUOC/uELEUocNOeF2LH0kgc64tOm97FHaHs2aqA="

[build]
builder = "autotools"
tests = true

[outputs]
default = ["out"]
```

A more interesting package can add:

```toml
[deps]
tools = ["pkg-config"]
libs = ["zlib"]

[[patches]]
path = "export-variable.patch"
sha256 = "sha256-..."

[build.env]
LDFLAGS = "--undefined-version"

[build.flags]
configure = ["--enable-shared"]
make = ["SHARED_MODE=1"]

[outputs]
default = ["out"]
all = ["out", "dev", "static"]
```

Exact spelling can change before implementation. The key point is that the
evaluated result is declarative and schema-bound.

### 7.2 Sources

Initial source kinds:

- `url`
- `git`
- `local`

All remote sources require digests or immutable commits. Mutable names such as
branches are not source identities.

### 7.3 Dependencies

For the first milestone, expose two common dependency classes:

- `tools`: executables needed while building the package
- `libs`: dependencies needed by the resulting artifact

Internally, store full platform roles:

- build platform
- host platform
- target platform

This keeps the v0 manifest simple while avoiding a later redesign when compiler
packages and cross-compilation arrive.

### 7.4 Builders

Initial standard builders should be few:

- `autotools`
- `cmake`
- `meson`
- `make`
- `cargo`
- `script`

The standard builders encode the boring conventions currently spread through
nixpkgs' `stdenv` and helper functions. The `script` builder is an explicit
escape hatch for packages outside those conventions, with script bodies stored
in separate source files rather than embedded in package metadata.

### 7.5 Outputs

BuckPkgs should support named outputs from the start because the split is cheap to
model early and expensive to retrofit later.

Likely initial names:

- `out`
- `bin`
- `dev`
- `lib`
- `doc`
- `static`

Manifests may declare only the outputs they actually use.

### 7.6 Runtime Closure

BuckPkgs should distinguish:

- inputs needed to build a package
- artifacts needed to consume package metadata during another build
- artifacts needed to run produced executables

The first implementation can keep runtime closure simple and explicit. It
should not silently rely on host libraries or ad hoc PATH lookup.

## 8. Identity, Storage, And Caching

### 8.1 Package Identity, Store Identity, Action Identity, And Output Identity

BuckPkgs needs four related but distinct identities.

A package instance should have a stable digest over:

- canonical manifest content
- builder implementation version
- resolved source digests
- patch digests
- direct dependency store paths grouped by role
- platform configuration
- declared relevant environment
- declared output set

That package-instance digest is useful for BuckPkgs analysis memoization, lockfile
explanations, diagnostics, and identifying semantically distinct instances.

Each declared output then gets a pre-build-known `StorePathKey` derived from the
package-instance digest, output name, and visible store name. That key names the
absolute `/pkgs/store/...` prefix packages may embed into their outputs.

Buck2 action digests remain the execution-cache identity. They are derived from
the actual remote action: command, inputs, environment, platform, and outputs.

The actual built outputs should be CAS directory digests. They are the transport
identity for RE, sharing, and materialization. The action cache maps action
digests to those output digests; BuckPkgs should not treat a package-instance digest
as a substitute for content identity.

### 8.2 Physical Storage

Buck2 artifacts, CAS, and the action cache should be the realization layer in
v0. `/pkgs/store/...` should be the logical package namespace that recipes and
dependents use.

Each output should have an inspectable logical path such as:

```text
/pkgs/store/<pkgs_path>
```

Those paths are backed by Buck2 artifacts and CAS output digests. Normal Buck2
consumers should still receive providers rather than resolve package names
manually. Remote execution should mount only the store-path closure needed by the
action.

The exact `pkgs_path` encoding is still open; see
[STORE_PATHS.md](./STORE_PATHS.md).

### 8.3 Binary Caches

The first binary cache is Buck2 remote execution plus the action cache. A core
integration requirement is that BuckPkgs package inputs and outputs upload cleanly
to RE CAS and participate in the existing action cache exactly like ordinary
Buck2 actions.

If BuckPkgs later needs standalone binary distribution, it can export/import package
instances by logical digest, but that is not required for the first buildable
system.

## 9. Buck2 Integration

### 9.1 Command Surface

Likely commands:

- `buck2 pkgs build <pkg>`
- `buck2 pkgs show <pkg>`
- `buck2 pkgs graph <pkg>`
- `buck2 pkgs import-nixpkgs ...`

Normal Buck2 targets should also be able to depend on package providers without
shelling out to a separate binary.

### 9.2 Configuration

Use Buck2 configurations rather than inventing BuckPkgs-specific platform logic.
Package instances should be configured nodes in the same sense that toolchains
and normal targets are configured nodes.

### 9.3 Consumption From Rules

Buck2 rules should consume package outputs through providers, not hard-coded
paths. Examples:

- a C++ rule receives include dirs and libraries from a BuckPkgs provider adapter
- a codegen rule receives a BuckPkgs executable as `RunInfo`
- a toolchain rule points at BuckPkgs-built compiler packages

### 9.4 Implementation Boundary

The intended final architecture is daemon-native:

- manifest parsing in Rust
- package graph and identity in Rust
- lowering to Buck2 actions through internal APIs
- a small Starlark-facing provider layer where Buck2 rules need to consume
  package outputs

A Starlark-only prototype is acceptable for experiments, but it should not
become the long-term architecture if the project goal is an integral Buck2 fork.

## 10. Porting From nixpkgs

The right use of nixpkgs is as a source corpus, not as an evaluator.

### 10.1 Porting Strategy

1. Manually port a bootstrap set and a few representative packages.
2. Identify recurring patterns in nixpkgs:
   - fetcher shape
   - standard builder
   - common dependency classes
   - patches
   - output splits
3. Keep the BuckPkgs package shape close enough that those ports stay quick to
   write and review by hand.
4. Add only small assistive tools for mechanical jobs such as hash-format
   conversion or patch-list extraction when they do not require evaluating Nix.

### 10.2 What Ports Cleanly

- straightforward `fetchurl` packages
- common autotools/cmake/meson packages
- packages that already use strict dependency separation
- simple multiple-output packages

### 10.3 What Should Not Be Recreated

- packages built around arbitrary Nix functions
- deep override chains
- recursive package-set tricks
- custom shell phases with heavy dynamic logic
- complicated cross-compilers in the first milestone

These are places to make explicit BuckPkgs choices, not targets for an automatic
import layer.

## 11. Bootstrap

Hermetic package managers still need a trust root.

BuckPkgs should follow the practical shape of nixpkgs' ordinary Linux bootstrap:

1. Start from a foreign Buck2 seed containing enough tools to build a useful
   system quickly.
2. Rebuild the base toolchain and userland in stages.
3. Require the final base toolchain closure to contain no references to the
   foreign seed or foreign toolchain outputs.

The seed must cover at least:

- shell
- archive tools
- compiler and linker
- libc
- patching tools
- basic build tools

The intended path is practical self-hosting, not `minimal-bootstrap`. See
[BOOTSTRAP.md](./BOOTSTRAP.md).

## 12. Proposed Repository Shape

```text
pkgs/
  README.md
  DESIGN.md
  OPEN_QUESTIONS.md
  MANIFEST_LANGUAGE.md
  BOOTSTRAP.md
  STORE_PATHS.md
  packages/
    ...
  schemas/
    ...
  bootstrap/
    ...
  tools/
    ...
```

Once implementation starts inside the Buck2 fork, likely Rust modules will live
under Buck2's `app/` tree, with prelude/provider glue where needed.

## 13. First Milestone

The first end-to-end milestone should be:

1. Pick the package-definition syntax after the manifest-language spike.
2. Parse package definitions into the Rust schema.
3. Resolve a tiny locked package-set composition.
4. Build a tool-first slice through Buck2 actions:
   - `gnused`
   - `gnugrep`
   - `gawk`
   - one package or Buck2 action that consumes them as hermetic tools
5. Expose one package to a normal Buck2 target as a provider.
6. Rebuild from a clean checkout with no undeclared host dependency except the
   explicitly accepted bootstrap set.

The next slice should add:

- `bash`
- one service used by tests, likely `postgresql`
- one native library relevant to Rust integration, such as `sqlite`
- then the first hermetic toolchain pieces, starting with `gcc`

If the first slice works, the architecture is real. Before that, broad package
coverage is a distraction.
