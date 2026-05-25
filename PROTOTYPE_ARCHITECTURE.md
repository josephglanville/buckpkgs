# Prototype Architecture

The first serious BuckPkgs implementation can probably be built from:

1. ordinary Buck2 Starlark rules and providers
2. a small set of externally bootstrapped Cargo binaries used as action tools

No Buck2 fork is required to validate the package model.
Package-authoring semantics exercised by the prototype are defined in
[PACKAGING.md](./PACKAGING.md); this document covers implementation placement.

## What Starlark Can Own

Normal Buck2 Starlark is sufficient for:

- package graph edges
- package/output providers
- dependency role propagation
- store-path computation
- lowering package builds to actions
- exposing runnable tools through `RunInfo`

Build-file Starlark already provides `sha256()`, so if package definitions are
available as Starlark data, BuckPkgs can compute store keys without daemon-side Rust
work.

The core rule family can stay small:

- `pkgs_package`
- `pkgs_fetch`
- maybe `pkgs_env`
- provider types such as `PkgsPackageInfo`

## What Cargo Binaries Should Own

Keep the complicated imperative behavior out of Starlark:

- source fetching
- archive unpacking
- patch application
- builder execution
- fixup passes
- immutable tree staging for native Buck2 store-output materialization

Expose that behavior as small Rust executables split by action surface, for
example:

```text
pkgs_configure_make_install
pkgs_make_install
pkgs_linux_headers_install
pkgs_stage_tree
pkgs_verify_no_refs
```

They can share Rust modules internally, but unrelated builders should not share
the executable identity that Buck2 bakes into action keys. Changing a headers
installer must not invalidate every autotools package build.

The binaries can be built with Cargo outside Buck2 during bootstrap and exposed
to Buck2 as executable tools.

## Manifest Choice

For the first prototype, package definitions are restricted Starlark
data/functions.

That keeps the graph pure normal Buck2 Starlark and avoids an out-of-band
generation step before Buck2 sees the package graph. The Starlark surface should
remain deliberately much smaller than arbitrary Buck2 rule authoring and should
lower to strict Rust-validated package data.

## Suggested Shape

```text
packages/*.bzl
  -> package descriptors
  -> pkgs_package(...) macros/rules
  -> Buck2 actions invoking pkgs-tool
  -> output trees under buck-out
  -> local realization into /pkgs/store
```

`pkgs_package` can:

1. receive fully declared attrs for sources, deps, outputs, builder, and hooks
2. compute `PackageInstanceDigest` and `StorePathKey` in Starlark
3. register build/fixup actions with `ctx.actions.run`
4. return `PkgsPackageInfo`, `DefaultInfo`, and `RunInfo` as appropriate

## The One Awkward Boundary

Realizing `/pkgs/store/...` is not a normal Buck2 output, because Buck2 outputs
are project-relative artifacts.

There are three plausible prototype choices:

### A. External Realizer

`buck2 build` produces package trees, then `pkgs-tool realize` installs them into
the local store.

- cleanest model
- easiest to reason about
- awkward if dependent package builds need the store during the same invocation

### B. Local Side-Effect Realizer Node

A Buck2 action invokes `pkgs-tool realize`, writes the immutable store path, and
also emits a stamp artifact.

- gives one graph for local bootstrapping
- likely acceptable for a prototype because store paths are immutable
- not perfectly Buck2-native because the real output is outside Buck2's tracked
  output tree

### C. Per-Action Store View

The builder tool constructs a private `/pkgs/store` view for each package build
from dependency artifacts.

- closest to the future RE model
- avoids global side effects
- more machinery up front

For "useful as fast as possible," **B** is probably the pragmatic first choice,
with **C** as the cleaner later direction.

If BuckPkgs does **not** want to accept **B** even temporarily, then this is probably
the first real reason to fork Buck2: teach it that a `/pkgs/store/...` path can
be a first-class realized output/materialization target rather than an
untracked side effect.

## What This Buys Us

This approach lets us validate:

- package definitions
- store-key design
- dependency roles
- nixpkgs porting strategy
- builders
- bootstrap graph

before committing to:

- Buck2 daemon changes
- RE-specific mechanics
- a fully native materializer/store integration

## Likely Later Fork Points

A Buck2 fork becomes valuable later for:

- first-class `/pkgs/store` materialization
- making store closures normal Buck2 inputs instead of prototype conventions
- automatic provider-aware propagation into arbitrary Buck2 actions
- better query/audit/explanation support
- cleaner local/remote parity

Those are important, but they are not prerequisites for proving the package
manager itself.
