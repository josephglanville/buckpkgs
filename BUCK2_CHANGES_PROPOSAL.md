# Buck2 Changes Proposal

This document proposes the Buck2 changes BuckPkgs wants once the package model is
stable enough to justify a fork.

The design goal is narrow:

1. BuckPkgs packages remain ordinary Buck2 graph nodes and actions.
2. `/pkgs/store/...` becomes a first-class package ABI, not an external
   realization hack.
3. Buck2 keeps ownership of scheduling, caching, CAS, materialization, and
   remote execution.

The proposal does **not** turn Buck2 into Nix. It teaches Buck2 one missing
concept: immutable absolute store outputs with declared store-path closures.

## 1. Design Thesis

BuckPkgs needs two properties that stock Buck2 does not currently combine:

- **Nix-like package prefixes**
  - package outputs have pre-build-known absolute paths such as
    `/pkgs/store/<key>-bash-5.3`
  - those paths may be embedded into interpreters, RPATHs, wrappers,
    `pkg-config` files, and generated scripts
- **Buck2-native execution**
  - packages are ordinary configured targets
  - package builds are ordinary actions
  - realized bytes remain ordinary CAS-backed artifact values

The current prototype can approximate this with:

- Buck2 outputs staged under `buck-out`
- package builders configured against logical `/pkgs/store/...` prefixes
- a BuckPkgs-specific local realization step that copies package trees into the
  host store

That is adequate for prototyping package semantics, but it is not the final
integration. The Buck2 fork should remove the ad hoc realization layer and make
store outputs native.

## 2. Core Properties We Want

The combined Buck2 + BuckPkgs system should have these properties.

### 2.1 Graph-Native Packages

Packages are normal Buck2 configured nodes:

- package dependencies are normal graph edges
- package outputs are normal providers and artifacts
- package builds register ordinary Buck2 actions
- downstream targets depend on package labels such as:

```text
//shells/bash:bin
//development/libraries/openssl:lib
```

BuckPkgs must not introduce a second scheduler or executor.

### 2.2 Stable Absolute Package ABI

Each BuckPkgs output has a pre-build-known path:

```text
/pkgs/store/<store-key>-<store-name>
```

This path is allowed to affect build outputs. Recipes may use it for:

- install prefixes
- shebangs
- dynamic loader metadata
- wrapper scripts
- `.pc` files
- config scripts

This is the main portability lesson taken from Nix.

### 2.3 Semantic Store Identity Separate From Buck2 Execution Identity

BuckPkgs should preserve four identities:

- `PackageInstanceDigest`
  - semantic identity of the fully resolved package instance
- `StorePathKey`
  - semantic identity of one output under `/pkgs/store`
- `ActionDigest`
  - Buck2/RE execution-cache identity for one lowered action
- `OutputDigest`
  - CAS digest of the produced output tree

Only `StorePathKey` belongs in the public store path.

The key invariant is:

> BuckPkgs package identity is computed from an unrendered semantic build plan.
> Store paths are rendered only after identity assignment.

That means package plans must distinguish two cases while computing identity:

- **self outputs** stay symbolic until this package's store identity is fixed
- **dependency outputs** are resolved before hashing to the dependency output
  identity/store path that the build will actually observe

For example, this:

```text
configure --prefix=${out} --with-bash=${bash.bin}
```

should hash as:

```text
SelfOutput("out")
ResolvedDependencyOutput(
    label = "//shells/bash:bin",
    store_path = "/pkgs/store/<dep-key>-bash-5.3",
)
```

Only the self reference is rendered after this package's identity is fixed.
Dependency store paths are already part of the package identity input because
they are visible to the build and may affect produced bytes.

This prevents identity cycles while still allowing packages to embed their final
absolute prefix freely, and it keeps the proposal aligned with the
`STORE_PATHS.md` rule that direct dependency store paths participate in identity.

### 2.4 CAS Deduplication Remains Available

Distinct store paths may realize to identical bytes:

```text
/pkgs/store/aaa-foo
/pkgs/store/bbb-foo
```

Those are distinct semantic identities, but Buck2 CAS can still store and
transfer identical file and tree digests once.

Therefore:

- `/pkgs/store/...` is the **semantic package namespace**
- CAS digests are the **content namespace**

The public ABI should stay `/pkgs/store/...`. A future local implementation may
have a backing CAS layout, but package recipes should not depend on paths such as
`/pkgs/cas/...`.

### 2.5 Declared Store Closures

Every action that consumes BuckPkgs packages should receive an explicit store-path
closure.

Locally:

- Buck2 can materialize the required store outputs under the global host
  `/pkgs/store`

Remotely:

- workers should receive only the declared closure required by the action
- those artifacts should be mounted at the same absolute `/pkgs/store/...`
  paths visible locally

No action should rely on ambient undeclared host store contents.

## 3. Proposed Buck2 Changes

### 3.1 Add A First-Class Store Output Path Kind

Today Buck2's built outputs are fundamentally project-relative build artifacts.
BuckPkgs needs another output namespace:

```text
Build output path =
  BuckOut(...)
  Store(...)
```

The exact Rust types may differ, but the meaning should be explicit:

- `BuckOut` outputs materialize under the project-relative output root
- `Store` outputs materialize under the configured absolute store root

This should be modeled as a real artifact/output path kind, not as a loose alias
table bolted on after action execution.

### 3.2 Add A Store Output Declaration API

BuckPkgs rules need an API roughly like:

```python
store_path = pkgs_store_path(...)
ctx.actions.declare_store_output(store_path = store_path, dir = True)
```

Ordinary users should not need to call this directly. `pkgs_package(...)` and
related helpers should wrap it.

The declaration API should enforce:

- the path is a typed, already-validated BuckPkgs store-path value, not an arbitrary
  user-authored string
- the typed path was derived from the BuckPkgs package/output identity contract
- the rendered path is under the configured BuckPkgs store root
- the path is known during analysis
- multiple outputs cannot claim the same store path inconsistently
- store outputs are treated as immutable realization targets

### 3.3 Split Path Resolution Into Three Contexts

One path answer is no longer sufficient. Store artifacts need three:

1. **Command-line/logical path**
   - what a package recipe sees
   - usually `/pkgs/store/<key>-pkg`
2. **Execution path**
   - the staging destination an executor writes to
   - project-relative or otherwise executor-native
3. **Materialization path**
   - where Buck2 exposes the final local output
   - `/pkgs/store/<key>-pkg`

Buck2 should make this distinction explicit rather than assuming one resolved
artifact path serves all consumers.

Conceptually:

```text
resolve_for_command_line(...)
resolve_for_execution(...)
resolve_for_materialization(...)
```

This preserves fixed absolute prefixes for package recipes without requiring
executors to write directly into `/pkgs/store`.

### 3.4 Keep Executors Staged, Not Store-Mutating

Executors should continue to produce action outputs through ordinary output
collection.

The flow should be:

1. analysis declares a logical store artifact
2. Buck2 assigns an internal execution/staging path
3. the action writes the staged tree
4. Buck2 captures the result as an `ArtifactValue`
5. the materializer realizes that artifact value at `/pkgs/store/...`

This keeps:

- output cleanup normal
- local and RE execution protocols closer to their current model
- `/pkgs/store` materializer-owned rather than action-owned

### 3.5 Teach The Materializer About Store Outputs

The materializer should own local realization of store outputs.

It should provide:

- idempotent realization of `/pkgs/store/...`
- reuse of already materialized matching outputs
- immutability checks or conflict detection
- repair/rematerialization from CAS when local state is missing
- coordination for concurrent realization attempts against the same global store
  path across builds and Buck2 daemon processes
- verification that an existing store path corresponds to the `ArtifactValue`
  Buck2 is trying to realize, rather than merely checking that the directory
  exists
- an explicit policy for nondeterministic collisions where the same semantic
  store path is claimed with different produced bytes

The materializer may use copying, linking, reflinks, or another local strategy.
That is an implementation choice. The semantic requirement is that the visible
store path reflects the artifact value Buck2 recorded.

This replaces the prototype's explicit "realize into `/pkgs/store`" action with
a native Buck2 responsibility.

The implementation may need a store index, lock sidecar, or equivalent metadata
owned by the materialization layer. The proposal should not assume filesystem
existence checks alone are sufficient for correctness.

### 3.6 Make Store Closures First-Class Inputs

BuckPkgs providers should expose enough metadata for Buck2 to know:

- which store outputs belong to a package
- which transitive store outputs are required at runtime, build time, or tool
  execution time

When an action depends on BuckPkgs packages, Buck2 should assemble the declared store
closure as part of the action input model.

Local semantics:

- required store paths are materialized before execution

Remote semantics:

- required store outputs are supplied from CAS
- the worker mounts them at their absolute `/pkgs/store/...` locations

This is the main remote execution integration point. It should be designed now,
even if implemented after the local store-output machinery.

Those store mounts are part of the action's execution semantics and therefore
must affect cache correctness. The desired rule is:

- keep Buck2's existing action-digest machinery
- extend the action payload/input model so the declared store closure, or an
  equivalent canonical representation of it, is committed into the action
  digest

The proposal is **not** that store mounts stay outside the RE cache key.

### 3.7 Surface Store Outputs In Buck2 UX

The integration should be visible to developers.

Useful follow-up work:

- output reporting should show `/pkgs/store/...` paths
- query/audit commands should expose store outputs and closures
- error messages should mention logical store paths, not only staging paths

This is not foundational, but it matters for debuggability.

### 3.8 Preserve Failed Package Build Environments

Package bring-up is expensive when every failed iteration loses the most useful
local state.

Separately from store support, the Buck2 fork should consider:

- preserving failed local action work directories when requested
- printing the replayable command and exact mounted inputs
- allowing a fast rerun of one failed action without rediscovering the whole
  failure from scratch

This is not required for correctness, but it would materially improve BuckPkgs
iteration speed.

### 3.9 Co-Develop The Native Authoring Model With The Fork

Several weaknesses in the current prototype authoring surface exist because
stock Buck2 cannot model store outputs directly. The fork work should remove
those pressure points rather than preserve them as permanent package authoring
constraints.

The current prototype has:

- a split between hidden tree-producing rules and separate `pkgs_package(...)`
  realization wrappers
- author-maintained `identity = "...recipe-vN"` strings
- dependency path interpolation through format strings such as
  `"--with-gmp={}"` and `"LDFLAGS=-Wl,-rpath,{}/lib"`
- prototype-only realization plumbing such as explicit realization roots and
  local store-entry paths threaded into builders
- no general typed self-output reference outside the install-prefix special case

Those are acceptable prototype compromises. They should not become the native
long-term authoring model.

The Buck2 fork should be developed and tested alongside a tightened BuckPkgs
authoring layer with these properties:

1. **Single package rule owns build plus store output**
   - once Buck2 can declare store outputs directly, package builders should stop
     generating a `__tree` target plus a separate realization wrapper
   - one package rule should own the action graph, store output, and providers
2. **Semantic identity is derived, not manually bumped**
   - the store identity input should be canonical structured recipe data:
     builder kind/version, normalized args, normalized env, patch digests,
     symlink/fixup config, declared outputs, source digests, and direct
     dependency store paths grouped by role
   - manual identity/version strings may remain only as a narrowly justified
     explicit escape hatch, not the normal correctness mechanism
3. **Dependency path references are typed recipe fragments**
   - replace ad hoc string templates with structured argument/env values that
     explicitly carry dependency-output references
   - package authors should express meaning like:

```text
arg("--with-gmp=", dep_path("//development/libraries/gmp:out"))
env("LDFLAGS", join([
  literal("-Wl,-rpath,"),
  dep_path("//development/libraries/gmp:out", "lib"),
]))
```

   - the exact surface can change, but the identity model must consume typed
     semantic refs rather than opaque formatted strings
4. **Self store-path references are first-class**
   - packages must be able to reference their own final output symbolically,
     not only through an implicit install-prefix field
   - that is required by the identity rule in section 2.3
5. **Realization mechanics disappear from recipe authoring**
   - package authors should not reason about realization roots, local host-store
     lookup paths, or prototype stamp artifacts
   - those are Buck2/materializer/store-closure concerns

This authoring cleanup is part of the fork plan because the native Buck2 store
model makes it both possible and necessary. If the fork lands without the
authoring model changing, BuckPkgs would still carry prototype-era stringly typed
semantics that undermine the identity guarantees the fork is meant to provide.

## 4. What Should Stay Unchanged

The fork should not disturb Buck2's core architecture unnecessarily.

These should remain conceptually unchanged:

- configured target identity
- graph edges and providers
- `ActionKey`
- action lowering
- action scheduling
- the overall RE action-digest mechanism
- CAS artifact values

BuckPkgs store paths are a new semantic namespace layered into the artifact and
materialization model. They should not replace Buck2's existing execution-cache
or CAS identities. The content being digested may still need to expand so
declared store-closure mount semantics participate in cache correctness.

## 5. Non-Goals For The Buck2 Fork

Do not use the fork to:

- add a Nix-like evaluator
- add a second package scheduler
- derive store paths from action digests
- derive normal package store paths from realized content digests
- let executors mutate `/pkgs/store` directly
- force package outputs to become relocatable Buck2 artifacts
- expose a public `/pkgs/cas/...` package ABI

Those all work against the model BuckPkgs is choosing.

## 6. Expected Combined-System Properties

If Buck2 gains the changes above, the combined system has:

1. **Buck2-native packaging**
   - packages are ordinary Buck2 graph nodes, actions, providers, and artifacts
2. **Nix-like absolute install prefixes**
   - final package prefixes are known before the build and may be embedded
3. **No identity cycles**
   - store identity is derived from symbolic package plans, not rendered self
     paths
4. **CAS-backed content dedupe**
   - logical store paths remain distinct while identical produced bytes dedupe
     underneath
5. **Hermetic local consumption**
   - downstream actions consume declared store closures, not host tools
6. **Remote-execution compatibility**
   - workers can mount only declared store closures at fixed absolute paths
7. **Native local store reuse**
   - repeated builds reuse materialized immutable package outputs
8. **A practical nixpkgs porting surface**
   - recipes retain the fixed-prefix assumption that many real packages expect
9. **A path to fully hermetic toolchains**
   - toolchains can consume BuckPkgs packages using the same declared-store model as
     ordinary tests and actions
10. **Recipe identity is difficult to get wrong**
    - normal package edits flow into the canonical semantic recipe hash without
      requiring manual identity bumps
11. **Authoring stays data-shaped**
    - dependency and self store-path references are typed recipe fragments, not
      opaque formatted strings

## 7. Suggested Implementation Sequence

The fork should proceed in this order:

1. generalize Buck2's built-output path model to include store outputs
2. add the store-output declaration API
3. add explicit command-line/execution/materialization path resolution
4. teach the materializer to realize store outputs under `/pkgs/store`
5. add local declared-store-closure input handling so package builds can consume
   prior store outputs natively
6. collapse the prototype tree-plus-realizer authoring split into native
   store-output package rules
7. derive store identity from canonical structured recipe data instead of manual
   `identity = "...recipe-vN"` strings
8. replace string-template dependency interpolation with typed dependency/self
   store-path recipe fragments
9. surface store outputs through BuckPkgs providers and Buck2 output reporting
10. extend RE action payloads/digests for store-closure semantics
11. add RE support for absolute store-path closure mounts
12. add failure-preservation and replay ergonomics for package bring-up

The first eight steps are the architectural hinge. After them, BuckPkgs stops
fighting Buck2's artifact model, package authors stop carrying prototype-era
realization workarounds, and nontrivial package graphs can be built locally with
a native store-output/store-input model and a canonical semantic identity model.

## 8. Relationship To Existing BuckPkgs Documents

This document is the fork proposal.

It relies on and refines:

- `STORE_PATHS.md`
  - identity model and store-path hashing
- `BUCK2_NATIVE_MODEL.md`
  - how package semantics map onto Buck2
- `BUCK2_STORE_INTEGRATION.md`
  - lower-level integration observations from reading Buck2 internals

Those documents explain the pieces. This one states the desired Buck2 change set
as a single proposal.
