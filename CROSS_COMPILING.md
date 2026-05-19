# Cross Compiling

## Purpose

BuckPkgs is already target-platform aware, but it is not yet a real
cross-compilation system.

Today the repo can select `linux-x86_64` vs `linux-arm64` target configurations,
and package identities already carry a target-system dimension. What is still
missing is a coherent model for:

- the platform running build actions
- the platform produced binaries execute on
- the platform compiler-like packages emit code for

This document defines the implementation direction for that missing layer.

## Design Summary

The design should follow two rules:

1. Use Buck2's configuration model where Buck2 already has the right concept.
2. Add GNU-style build / host / target semantics only at the BuckPkgs package
   layer where package builders and bootstrap logic actually need them.

In practice:

- Buck2 target platforms remain the source of truth for produced artifacts.
- Buck2 execution platforms, `exec_dep`, and `toolchain_dep` remain the source
  of truth for tools that run during the build.
- BuckPkgs exposes canonical package-facing platform metadata derived from those
  configurations, including triples suitable for Autotools and toolchain
  bootstrap logic.
- Ordinary packages should get ergonomic defaults.
- Compiler/bootstrap packages may opt into more explicit cross machinery.

## Current State

The important pieces already exist:

- target platform selection in `platforms/BUCK`
- package dependency roles in `rules/pkgs.bzl`
- `configure_args` and `configure_env` plumbing in `rules/pkgs.bzl`
- staged native GCC, binutils, and glibc graphs
- CC and bintools wrapper generation in `crates/pkgs-tool`

The gaps are equally concrete:

- package authoring does not expose canonical build / host / target triples
- configure flag emission is still handwritten
- `target_inputs` do not materially shape actions yet
- the GCC/binutils/glibc graph is a native staged bootstrap, not a cross
  bootstrap
- wrapper responsibilities are not described in machine-role terms
- validation still contains target-specific assumptions such as the hardcoded
  x86_64 ELF interpreter path in `bootstrap/tests/BUCK`

## The Buck2 Boundary

### Buck2 Concepts To Reuse

Buck2 already separates:

- **target platform**: the platform the configured target is built for
- **execution platform**: the platform used to run build actions
- **execution dependencies**: tools that must themselves be configured for the
  execution platform
- **toolchain dependencies**: a structured provider boundary that bundles
  execution tools plus target-specific configuration

That maps cleanly onto BuckPkgs:

- package outputs follow the Buck target platform
- build tools follow the Buck execution platform
- compiler/tool wrappers should be assembled through toolchain-like providers,
  not by treating every package dependency as equivalent

### What Buck2 Does Not Provide

Buck2 does not by itself model GNU Autoconf's three-way distinction:

- `build`
- `host`
- `target`

That distinction belongs in BuckPkgs because it is package-system semantics, not
generic build graph semantics.

The correct layering is therefore:

```text
Buck2 target platform + execution platform
                  |
                  v
        BuckPkgs platform provider
                  |
                  v
 build / host / target triples for package rules
```

## Package-Facing Platform Model

### Canonical Fields

BuckPkgs should introduce one canonical provider or helper surface that exposes:

- `build_triple`
- `host_triple`
- `target_triple`
- structured platform records where needed, such as CPU, OS, ABI, libc, and
  interpreter metadata

Recommended semantics:

- `build_triple`: the platform running package build actions
- `host_triple`: the platform the resulting package executes on
- `target_triple`: the platform emitted code is meant for, only meaningful for
  compiler-like packages

### Native And Cross Cases

Representative examples:

```text
native package on x86_64 Linux
  build  = x86_64-unknown-linux-gnu
  host   = x86_64-unknown-linux-gnu

ordinary cross-built package for arm64 Linux
  build  = x86_64-unknown-linux-gnu
  host   = aarch64-unknown-linux-gnu

cross compiler running on x86_64 Linux and targeting arm64 Linux
  build  = x86_64-unknown-linux-gnu
  host   = x86_64-unknown-linux-gnu
  target = aarch64-unknown-linux-gnu
```

### Design Choice

Triples should be derived values, not repeated string literals in BUCK files.
Packages may inspect them, but ordinary package authors should not have to
reconstruct them.

## Autotools Cross Surface

### Decision

BuckPkgs should follow an ergonomics-first model similar to the one proven in
Nixpkgs:

- ordinary cross builds synthesize `--build=...` and `--host=...`
- `--target=...` is opt-in for compiler-like packages
- packages retain an escape hatch to override or suppress platform flag
  generation when upstream configure logic requires it

### Proposed Authoring Surface

One practical shape is:

```python
configure_platforms = ["build", "host"]
```

with compiler/bootstrap packages using:

```python
configure_platforms = ["build", "host", "target"]
```

Possible defaults:

- native package builds: `[]` or a normalized default that emits nothing
- ordinary cross package builds: `["build", "host"]`
- compiler/bootstrap helpers: explicitly request `target`

### Why This Fits The Current Repo

Current recipes already centralize configure behavior in:

- `development/compilers/gcc/BUCK`
- `development/tools/misc/binutils/BUCK`
- `development/libraries/glibc/BUCK`

They should not each hand-assemble triple strings. The rule layer should lower
the selected `configure_platforms` value into structured configure arguments.

## Dependency Role Contract

The current rule API already has:

- `native_build_inputs`
- `build_inputs`
- `target_inputs`
- `runtime_inputs`

Cross compilation should make these roles semantically crisp.

### Contract

`native_build_inputs`

: Tools that execute during the package action.

: These affect the action environment directly, including `PATH`.

`build_inputs`

: Build-machine material required by those tools.

: These are available to the action, but are not themselves executable tools
  by convention.

`target_inputs`

: Target-machine headers, libraries, sysroots, CRT objects, and ABI material
  used to produce the package artifact.

: These must stop being mostly metadata. They need to materially participate in
  configure, compile, or wrapper construction.

`runtime_inputs`

: Runtime closure of the final package output.

### Immediate Consequence

The first cross implementation should update at least one real build path so
`target_inputs` change emitted action arguments or wrapper construction. Until
that happens, the dependency vocabulary is ahead of the behavior.

## Sysroots And Bootstrap Ownership

### Decision

Sysroot composition is mostly a bootstrap and toolchain-construction concern,
not ordinary package authoring.

The graph that decides a sysroot should decide:

- target headers
- libc
- CRT objects
- compiler runtime pieces
- compatible binutils

Ordinary packages should consume that result through a provider, not assemble it
ad hoc from loose dependencies.

### Recommended Provider Boundary

Introduce a toolchain/sysroot provider that can carry resolved information such
as:

- compiler executables
- target sysroot path
- target headers path
- libc path
- dynamic linker path
- associated triples

That provider should be constructed by toolchain/bootstrap rules and consumed by
wrappers and package builders.

## Wrapper Responsibilities

### CC Wrapper

The CC wrapper is already conceptually close to the cross model. It injects:

- `--sysroot`
- binutils search paths
- CRT search paths
- headers
- dynamic linker behavior
- runtime library search paths

The missing part is not capability. It is making sure the inputs it receives are
resolved from an explicit machine-role model:

- compiler executable runs on the build side
- sysroot, libc, headers, and interpreter belong to the target side

### Bintools Wrapper

The current bintools wrapper strips `--sysroot` from `ld`.

That behavior may be appropriate for the current native bootstrap, but it is not
obviously valid once BuckPkgs intentionally links against a foreign target
sysroot.

This should become an explicit design decision during the cross-toolchain pass:

- either preserve `--sysroot`
- or make sysroot stripping an explicit wrapper mode with a narrow reason

### Decision On Metadata Leakage

Wrappers should generally receive resolved paths and flags, not own the full
build / host / target reasoning themselves.

The role logic belongs in Buck/Starlark providers. Rust wrapper generators should
mostly render already-decided behavior.

If wrappers need to understand whether a path is build-side or target-side to be
correct, that is a signal the provider boundary is incomplete.

## Bootstrap Shape

### Required Cross Sequence

A real cross toolchain bootstrap needs a dedicated staged graph, along the usual
line:

1. cross binutils for the target
2. minimal cross GCC
3. target headers
4. target libc
5. fuller cross GCC

The current GCC/binutils/glibc rules are not that graph yet. They are native
staging rules.

### How Much Should Be Specialized

Bootstrap-specific machinery should stay low in the stack.

Ordinary packages should know:

- dependency roles
- platform-aware configure behavior
- whether they consume a resolved toolchain/sysroot provider

Ordinary packages should not know:

- stage0 vs stage1 bootstrap choreography
- how to interleave GCC, binutils, headers, and libc
- how sysroot construction is assembled internally

If those concepts start leaking into common package declarations, that is the
point to introduce more dedicated bootstrap macros or providers.

## Validation

Validation must follow the same target model as package construction.

The immediate fix is to make ELF interpreter expectations target-aware instead of
hardcoding:

```text
lib/ld-linux-x86-64.so.2
```

in `bootstrap/tests/BUCK`.

Before cross work goes much farther, tests should be able to select the expected
interpreter by target platform.

## Implementation Order

1. Add canonical platform/triple helpers and platform-aware interpreter
   metadata.
2. Add ergonomic Autotools cross plumbing that can synthesize `--build`,
   `--host`, and optionally `--target`.
3. Tighten the semantics of `target_inputs` and use them in one real package or
   wrapper path.
4. Prove the model with a small arm64 cross-built package or fixture emitted from
   the existing x86_64 execution environment.
5. Build the dedicated cross binutils / GCC / headers / libc bootstrap graph.
6. Revisit wrapper behavior once the cross sysroot model is real, especially the
   `ld --sysroot` handling.

## Non-Goals For The First Implementation

The first cross-compilation implementation does not require:

- arm64 execution platforms
- arm64 workers
- heterogeneous remote execution pools
- execution-platform cache partitioning

Those matter later. They are not required for:

```text
run x86_64 build actions
emit arm64 package outputs
```

## Acceptance Criteria

The implementation is on the right track when:

- BUCK files do not hand-write triples for ordinary cross cases
- Autotools packages get consistent platform flags from shared rule logic
- package dependency roles have observable behavioral effects
- a foreign-target package can be built under the current execution platform
- ELF validation is target-platform aware
- wrapper inputs are explicit enough that sysroot ownership is not ambiguous
- the repo has a clear path to a dedicated cross compiler bootstrap graph

## Open Questions To Settle During Implementation

1. What exact provider shape best carries derived platform records and triples?
2. Should native builds emit no configure platform flags, or should the rule layer
   normalize them for uniformity?
3. Which first package or fixture gives the smallest convincing proof that
   `target_inputs` are semantically real?
4. Is `ld --sysroot` stripping still needed once cross sysroots become explicit?
5. At what point does repeated bootstrap logic justify dedicated macros instead
   of ordinary package declarations plus providers?
