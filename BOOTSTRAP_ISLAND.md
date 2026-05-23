# Bootstrap Island

## Problem

Before the substitute import surface, the package-backed C/C++ toolchain made
ordinary targets walk directly into the bootstrap turnover graph:

```text
toolchains//:cxx
  -> root//development/compilers/gcc:bin_stage2
  -> root//development/tools/misc/binutils:bin_stage1_wrapped
    -> bootstrap package tower
```

That is technically valid, but it is the wrong default shape:

- a small first-party C/C++ build can trigger the entire compiler bootstrap
- analysis and execution for ordinary work inherit the rebuild cost and failure
  modes of bootstrap work
- the bootstrap turnover graph stops behaving like infrastructure and starts
  behaving like an ambient dependency of the whole repository

The desired rule is stricter:

> Ordinary builds consume an already-finalized bootstrap closure. They do not
> implicitly rebuild or re-evaluate the bootstrap turnover graph.

## Design Goal

Split BuckPkgs into two graph surfaces:

1. **Bootstrap island**
   - owns the foreign seed
   - owns staged turnover packages
   - owns reproducibility validation and archive publication
   - may be expensive and intentionally rebuilt only by bootstrap workflows
2. **Ordinary package/build graph**
   - consumes a finalized bootstrap closure through already-hydrated store
     objects plus thin imported-provider declarations
   - never reaches bootstrap internals by default
   - fails clearly when the required finalized closure is unavailable

This is a dependency-graph boundary, not merely a cache preference.

## Boundary Shape

The clean end state is a dedicated Buck2 cell or equivalently hard-separated
namespace. The current implementation establishes the rule boundary under
`root//bootstrap`; a dedicated cell remains an enforcement improvement:

```text
bootstrap//...
  private staged packages
  private foreign seed edges
  reproducibility checks
  substitute archive exporters

root//...
toolchains//...
  import-only views of finalized bootstrap outputs
  ordinary package graph
```

Only a narrow exported surface crosses the boundary:

```text
bootstrap//exports:manifest
bootstrap//exports:gcc
bootstrap//exports:binutils
bootstrap//exports:glibc
bootstrap//exports:bash
...
```

The exported targets describe finalized immutable store objects. They must not
expose turnover-stage package dependencies to ordinary consumers.

## Immediate Policy Change

The repository should distinguish:

- **bootstrap compiler toolchain**
  - minimal default compiler used to build repo-local bootstrap helpers
  - currently the existing host/system-style bootstrap CXX toolchain
- **package-backed finalized compiler toolchain**
  - explicit opt-in consumer of imported finalized GCC/binutils/glibc store
    objects
  - not the ambient default until substitute import is in place and cheap

Concretely, the current default package-backed `toolchains//:cxx` is the wrong
long-term default because it makes a normal C++ target a bootstrap driver.
The better surface is:

```text
toolchains//:cxx_bootstrap
toolchains//:cxx_pkgs
```

with `cxx_pkgs` backed by imported finalized bootstrap outputs, not by live
stage labels. Targets that are explicitly testing or consuming the BuckPkgs GCC
toolchain can select `cxx_pkgs`. Repo-local bootstrap machinery keeps using
`cxx_bootstrap`.

## Import Surface

Ordinary builds need package providers with the same useful shape as built
packages:

- logical store path
- store artifact
- runtime closure
- runtime store outputs
- metadata such as package name/version/output

That suggests an import rule family, conceptually:

```python
pkgs_imported_store_output(
    name = "gcc",
    store_entry = "...-gcc-...",
    manifest = "//bootstrap/substitutes:gcc_manifest",
)
```

or, once archive substitution is first-class, a higher-level rule that takes a
closure manifest and emits the same `PkgsPackageInfo` provider the live package
rule emits.

The ordinary graph must not care whether the store object originally came from:

- a prior island rebuild
- a local substitute cache
- a remote substitute cache

It should see only immutable package providers.

## No Silent Live-Build Fallback

The default consumer path should not say:

```text
if substitute missing:
  rebuild bootstrap tower in place
```

That recreates the coupling this design is trying to remove.

The default consumer policy should be:

```text
if substitute missing:
  fail with the missing store object and the explicit bootstrap build/publish command
```

An explicit developer escape hatch can exist, but it must be a deliberate mode,
for example:

- a dedicated target platform
- a separate toolchain label
- a clearly named bootstrap development command

It should never be the ordinary dependency-resolution path.

## What Belongs In BuckPkgs

BuckPkgs should own:

- package identity and store-path keys
- which bootstrap outputs are exported
- closure manifests for those outputs
- the explicit bootstrap hydration workflow that fetches substitute bytes from a
  configured artifact service such as a CAS-backed cache
- package-level verification that exported closures contain no foreign-seed
  references
- archive publication targets
- imported-provider declarations that rebuild `PkgsPackageInfo` from substitute
  metadata without turning ordinary builds into substitute-fetch executions

The implemented pieces of that surface now include:

- `pkgs_export_store_substitute(...)`
- `pkgs_export_store_tree_substitute(...)`
- `pkgs_export_store_closure(...)`
- `pkgs_prebuilt_store_substitute(...)`
- `pkgs_imported_store_output(...)`
- `pkgs_hydrate_store_object`
- `pkgs_hydrate_store_closure`
- `pkgs_hydrated_store_output(...)`

`root//bootstrap/exports:linux_x86_64_bundle` publishes finalized live wrapper
roots. `root//bootstrap/substitutes:*` holds reviewed pinned closure/object
metadata and import-only provider declarations. Canonical package-facing labels
such as `root//development/libraries/glibc:out` and
`root//development/compilers/gcc:bin` alias those imports, while explicit
`*_stage*` and `*_final` targets remain live publication producers.

The imported bootstrap surface now also includes the final Bash, GNU Make,
Coreutils, Findutils, GNU sed, GNU grep, GNU awk, and GNU patch outputs required by ordinary
configure/make-style packages. `root//development/libraries/zlib:out`
demonstrates the boundary with a useful non-toolchain package: zlib is a direct
PostgreSQL build input in nixpkgs, builds shared and static libraries plus
`zlib.pc`, and names ordinary package labels whose canonical definitions resolve
to pinned bootstrap imports rather than live turnover or foreign-seed targets.
Higher-layer tools such as Ninja and Meson are normal native package
derivations built on that sealed imported façade. Their immediate Python
dependency is an explicitly reduced native build interpreter with `zlib`; its
GNU awk/grep/patch build-tool dependencies now also resolve through the pinned
imported surface, rather than reaching the live promotion graph. This is not a
claim that full canonical Python has already been built. Meson consumers remain
ordinary package recipes: a local fresh `inih` build has now completed through
the imported façade, rebuilding normal-layer Python and Ninja without entering
bootstrap producers. The newly pinned expanded closure has passed hydration
verification from its assembled bundle into an independent disposable store
root; a clean-consumer rerun after external distribution remains operational
validation.

Live producers and foreign seed wrappers are now restricted by an exact
`BOOTSTRAP_PRODUCER_VISIBILITY` allowlist, and substitute targets may be
consumed only by canonical façade aliases or bootstrap tests. A dedicated cell
would provide a still stronger namespace boundary. The live closure export
bundle is likewise not a public dependency surface; it can be selected for the
publication workflow and consumed by bootstrap validation targets only.
Producer-side seed-check targets and individual export actions are restricted
as well, since either otherwise provides an ordinary dependency bridge into
the live graph. Promoted GNU awk and GNU grep production recipes now consume
their published aliases instead of stage-zero self-hosting inputs. An isolated
run of the repaired recipes produced and pinned the published
`b060b888...-gawk`, `d23256b4...-gnugrep`, and
`f8debe78...-patch` generation without selecting the live closure bundle; once
those aliases advance, the private recipes correctly identify a subsequent
publication candidate rather than becoming normal-build dependencies. The
remaining operational work is publishing the verified expanded closure through
an authenticated remote channel and rerunning clean-consumer execution gates
against that published distribution.

## What Belongs In Buck2

The Buck2 fork already has the right local foundation:

- `ctx.actions.store_path(...)`
- `ctx.actions.declare_store_output(...)`
- staged build outputs that carry a logical `/pkgs/store/...` path
- materializer-driven publication into `/pkgs/store`
- verification that an existing store path matches the recorded artifact value
- atomic publication through a temporary path followed by rename, preserving
  and validating modes already sealed by the native package producer rather
  than mutating either new or reused published paths

The island design relies on that machinery, but it does not require ordinary
consumer targets to know anything about bootstrap turnover.

Buck2 changes that would make the island robust rather than merely conventional:

1. **Store-aware imported-provider support**
   - represent already-hydrated store objects cleanly in the ordinary graph
   - surface crisp diagnostics when the required closure is absent
   - implemented in source through a native import action that consumes the
     pinned source manifest directly, validates its declared provider contract
     and canonical tree bytes, and constructs its Buck artifact value in one
     physical traversal
2. **Store-aware diagnostic surface**
   - missing finalized store object errors should name the logical
     `/pkgs/store/...` path and the missing manifest entry
3. **Compatible imported-tree identity**
   - publish a Buck2 directory digest set or durable verified receipt alongside
     substitute metadata
   - implemented for the complete 17-object published substitute closure
     through reviewed REAPI directory digests served by local Foundry and
     Buck2's store-aware CAS import action
   - a first fresh `toolchains//tests:gcc_smoke` import normalized and sealed
     legacy physical objects while transferring `761MiB`; a subsequent fresh
     daemon verified and reused the sealed package-backed toolchain locally
     with no reported CAS transfer
   - retain the local authenticated walk on daemon restart until durable store
     receipts can safely elide it;
     the prior legacy-manifest verification plus Buck2 fingerprint duplicate
     walk has already been collapsed in source
   - imported store action outputs are content-addressed and use source
     manifests as inputs; native package build tools are execution
     dependencies, so the usual ordinary package and package-backed toolchain
     consumers share the same imported action nodes
   - address the repeated imported-object verification observed under the old
     importer in the `34.3s` first-use ordinary zlib/toolchain-smoke build;
     after the execution transition the combined path performed one imported
     GCC validation in `3.9s` and repeated hot in `0.0s`; keep separate the
     `9:58.7` cold `inih` run dominated by ordinary Python/Ninja rebuilding
4. **Remote execution parity**
   - imported finalized store closures should mount at the same logical paths
     when actions run remotely

## Phased Migration

### Phase 1: Stop New Coupling

- keep repo-local bootstrap helper binaries on the bootstrap CXX toolchain
- stop using live staged GCC turnover labels as the default ordinary CXX path
- introduce explicit `cxx_pkgs` naming for the finalized BuckPkgs toolchain

### Phase 2: Define The Exported Bootstrap Contract

- list the exact finalized store outputs ordinary builds may import
- define a closure manifest format
- make all exported bootstrap outputs pass:
  - reproducibility verification
  - foreign-seed reference checks
  - closure completeness checks

### Phase 3: Import Instead Of Rebuild

- an explicit hydrator fetches substitute blobs from the chosen artifact service
  and atomically realizes the finalized bootstrap closure
- ordinary graph consumes already-realized imports plus substitute metadata
  through Buck2's native no-copy import path
- missing substitute data fails locally and clearly
- bootstrap island workflow produces/publishes substitutes independently

### Phase 4: Tighten Graph Enforcement

- move staged bootstrap turnover behind a dedicated cell or equivalent hard
  namespace
- expose only import/export targets across that boundary
  - implemented for current targets with explicit producer/substitute
    visibility allowlists
- add lint or visibility checks so future additions cannot reach staged
  bootstrap internals accidentally

## Acceptance Criteria

The island is real when all of the following hold:

1. Building a normal first-party target does not analyze or execute turnover
   packages unless an explicit bootstrap-dev mode is selected.
2. Building with the finalized BuckPkgs GCC toolchain reads imported store
   objects instead of stage labels.
3. Missing bootstrap substitutes produce a targeted import error, not a surprise
   multi-hour compiler rebuild.
4. The bootstrap island can still be rebuilt from source in a dedicated workflow.
5. Rebuilt island outputs can be exported and then consumed by ordinary builds
   without changing their visible store paths or providers.
