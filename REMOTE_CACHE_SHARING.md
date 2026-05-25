# Remote Execution And Cache Sharing

## Scope And Decision

BuckPkgs and Foundry remain separate repositories. Foundry's intended integration
boundary is a pinned BuckPkgs external cell at an immutable, exportable revision,
not a merged source tree.

That boundary does not require arbitrary live Buck2 action-cache sharing across
consumer repositories. The primary cross-repository reuse mechanism is published,
reviewed BuckPkgs store content imported from Foundry CAS. Live remote execution
and action-cache reuse should initially be validated against one canonical
BuckPkgs graph.

Keep three workstreams separate:

1. Remote execution support for actions that consume declared BuckPkgs store
   inputs.
2. Store/cache identity correctness and validation.
3. Portable live action-cache keys across independent Buck2 graphs.

Workstreams 1 and 2 are needed for a reliable external-cell integration.
Workstream 3 is an optional optimization only if multiple repositories must
share cache hits for live, unpublished package builds.

## Current State

| Capability | Status | Current implementation |
| --- | --- | --- |
| Native `/pkgs/store` outputs and collision-safe materialization | Implemented | Buck2 native store output declaration, sealing, and existing-output validation |
| Published substitute import through CAS | Implemented | `pkgs_cas_store_output(...)` and pinned REAPI directory manifests; the normalized bootstrap closure has 24 role-specific CAS-published objects |
| Remote transport for declared store inputs | Implemented | Buck2 sends `buckpkgs.store_mounts.v1` command platform properties with staged-input to logical-store mappings |
| Remote worker realization of store inputs | Implemented | Foundry validates the mapping and mounts staged inputs read-only under `/pkgs/store` in Bubblewrap |
| Foundry action-cache mechanism | Implemented | Only successful cacheable build actions are stored or reused; legacy failed entries are refused |
| Declared runtime support for ambient bootstrap tools | Implemented and validated locally | The selected Foundry Bubblewrap profile `buckpkgs-bootstrap-v5` mounts the declared Cargo, Rustup, local-bin, Python, GCC/system-library, and LLVM 21 runtime trees required by the tested graph |
| BuckPkgs remote-enabled execution platform | Implemented | `platforms//:remote` selects remote execution and cache upload/use with the Foundry runtime profile property |
| End-to-end remotely executed ordinary package with store dependencies | Validated locally | On May 24, 2026, native BuckPkgs and Foundry's pinned `buckpkgs` external cell each remotely built `development/libraries/zlib:lib`; a native rebuild after daemon/materialization clearing reported `100%` Foundry cache hits |
| Portable live action keys across unrelated external-cell roots | Not implemented; measured miss | Native `zlib` lowered to `3048a3ba...:175`; Foundry's external-cell build lowered to `b2858fa9...:175` and executed 147 remote commands with only `1%` cache hits |
| Untrusted substitute publication | Not implemented | Current trust model is reviewed repository-pinned manifests, not signatures or an authenticated publication channel |

There are three distinct identities involved:

| Identity | Purpose | Required property |
| --- | --- | --- |
| Package instance digest / logical store path | Names the immutable package result | Every declared choice that can affect installed bytes or closure semantics changes the identity |
| Output or CAS digest | Identifies actual published bytes | A claimed logical store path must materialize exactly the expected object |
| Buck2 action digest | Determines remote action-cache reuse | It may conservatively miss; it must not incorrectly reuse an action with different inputs or behavior |

Published substitutes provide byte reuse even when action digests are not
portable. Portable action keys matter only for sharing *live build* results
between independently rooted graphs.

## Workstream 1: Remote Execution

### Implemented

- Buck2 represents declared store outputs and imported CAS store objects as
  native sealed store artifacts.
- Buck2 no longer rejects remote or hybrid execution merely because an action
  consumes declared store inputs.
- Buck2 publishes store mount mappings in `buckpkgs.store_mounts.v1` for
  remote execution.
- Foundry parses and validates that protocol and exposes each staged input at
  its logical `/pkgs/store/...` location through a read-only sandbox mount.
- Foundry selects explicitly named, versioned Bubblewrap runtime mount
  profiles through an action property. The tested v5 profile includes LLVM 21
  because Foundry's consuming Rust toolchain invokes `clang++`.
- Foundry materializes writable remote action workspaces, cleans up read-only
  sealed result trees across server restarts, and does not cache failed action
  results.
- BuckPkgs supplies `platforms//:remote`, which selects remote execution and
  the required runtime profile for live validation.

### Validation Performed

On May 24, 2026:

1. A native BuckPkgs build of `//development/libraries/zlib:lib` ran its
   `pkgs_configure_make_install` action through Foundry using declared
   `/pkgs/store` inputs and completed successfully under
   `buckpkgs-bootstrap-v5`.
2. Foundry built the same package through its pinned external cell,
   `buckpkgs//development/libraries/zlib:lib`, through the same remote
   service and completed successfully with the same runtime profile.
3. After clearing native Buck2 daemon/materialization state, the native build
   retrieved all 147 eligible commands from Foundry (`100%` cache hits);
   `zlib` retrieved action digest
   `3048a3ba5fdc4691ea6065697d0ce6e92a99d5d4eef3d1fd58a90613975832e4:175`.
4. The external-cell run did not reuse the native live package action. This is
   expected until Workstream 3 canonicalizes consumer-dependent action paths.

### Remaining RE Work

1. Turn the manually verified first remote execution and subsequent Foundry
   action-cache retrieval after local materialization clearing into automated
   integration coverage.
2. Run one larger representative package stack, such as Bubblewrap or
   PostgreSQL, to cover more realistic transitive store closure use.
3. Preserve negative checks: undeclared or malformed store mounts must be
   rejected, and existing logical-store collisions must remain errors.
4. Replace the local development runtime mount list with a deployment policy
   that provisions or imports every required tool runtime explicitly.

Importing an already-published CAS substitute is useful validation of the
distribution path, but it is not a substitute for remotely executing a live
package action that consumes declared store inputs.

## Workstream 2: Cache Identity Correctness And Validation

### Implemented

- `PACKAGING.md` defines immutable public store outputs, output roles,
  dependency-role contracts, split-output behavior, and package authoring
  policy.
- Package instance hashing commits structured recipe semantics including
  sources and patches, target system, role-aware direct dependencies, selected
  and split outputs, fixup policy, relevant metadata handling, job settings,
  and declared tool/reference/link relationships.
- Published CAS substitute manifests verify logical identity, expected package
  metadata, target system, canonical reference metadata, payload data, decoded
  tree content, and closure reachability before store import.
- Buck2's store materialization rejects collisions and seals successfully
  published objects.
- Foundry rejects undeclared store-mount targets, selects extra runtime mounts
  only by the requested versioned profile, provides writable sandbox inputs to
  build commands, cleans read-only sandbox output trees safely, and excludes
  unsuccessful actions from its action cache.

These controls are the correctness foundation. They protect both local and
remote builds, independently of whether a remote action-cache hit occurs.

### Remaining Correctness Work

1. Make byte-affecting builder implementation identity enforceable. Builder
   strings such as `configure-make-install-v18` currently carry this
   responsibility manually. Establish a tested policy requiring a builder
   identity or `STORE_ABI_VERSION` bump when shared builder/fixup behavior can
   alter installed bytes or closure semantics; a derived implementation
   fingerprint may replace that convention later.
2. Add focused identity mutation tests covering at least source/patch changes,
   builder identity, dependency roles, output selection/splitting, debug
   preservation, metadata-prefix relocation, job or platform settings that
   affect output, and ABI-version changes.
3. Add or consolidate integration checks showing that a changed mounted store
   dependency cannot reuse the old remote action and that malformed,
   mismatched, or colliding store imports fail.
4. Keep reviewed repository pins as the current trust boundary. Add signatures
   or an authenticated publication channel before accepting manifests from an
   untrusted service.

Avoiding a first-use CAS import traversal with reusable directory-digest
metadata or a durable trusted receipt is a performance improvement, not a
correctness requirement.

## Workstream 3: Cache-Key Portability

### Current Position

Defer portable live action-cache keys.

The implemented protocol transports store inputs safely, but Buck2 currently
includes staged store-input paths in the action salt. Two unrelated repositories
can therefore miss each other's live action-cache entries even when they
ultimately consume the same logical store objects. That is conservative and
correct.

It is also acceptable for Foundry's planned external-cell integration:
published BuckPkgs substitutes and immutable pinned store objects already
provide the high-value cross-repository reuse boundary without requiring live
rebuilds in each consumer graph.

### Observed Portability Miss

The May 24, 2026 validation used one Foundry CAS/action-cache namespace and the
same runtime profile:

| Build graph | Store-action digest | Representative staged input prefix |
| --- | --- | --- |
| Native BuckPkgs root | `3048a3ba5fdc4691ea6065697d0ce6e92a99d5d4eef3d1fd58a90613975832e4:175` | `buck-out/v2/art/root/bootstrap/substitutes/...` |
| Foundry with `buckpkgs` external cell | `b2858fa90887e3a84d5180bcd0076005142900ad983d3c78dacfce9a24724af5:175` | `buck-out/v2/art/buckpkgs/bootstrap/substitutes/...` |

The action identity also contains the configured action owner
(`root//...` versus `buckpkgs//...`). Thus the current miss is not a Foundry
cache failure and not a store-identity correctness failure. It is the expected
result of non-portable Buck2 action lowering.

### Trigger For This Work

Implement portability only if there is a concrete requirement for independent
repositories to share live remote build results for unpublished packages, not
merely consume published BuckPkgs outputs.

If that requirement appears:

1. Canonicalize every action-observable store input path, tool path, source
   path, command argument, environment entry, output location, and execution
   platform property that can vary with the consuming repository root.
2. Remove staged-path contributions from action identity only after the
   executor observes store inputs exclusively at canonical logical paths.
3. Add a two-root validation in which different external-cell aliases or
   checkout roots produce the same action digest and a remote cache hit for
   the same package instance.
4. Retain a negative validation in which any byte-affecting or semantic input
   variation produces a miss.

## Revised Sequence

1. Keep reviewed CAS substitutes and immutable store imports as the
   cross-repository distribution boundary.
2. Keep `platforms//:remote` and the Foundry v5 runtime profile validation
   path working, and add automated remote action-cache retrieval coverage.
3. Validate a representative ordinary package stack remotely once the `zlib`
   proof is covered in automation.
4. Close the remaining identity-policy gap with builder/ABI enforcement and
   mutation tests; require these checks for byte-affecting packaging changes.
5. Publish or identify an immutable exportable BuckPkgs revision and let
   Foundry consume it as a pinned external cell using published substitutes,
   without depending on a live local BuckPkgs checkout.
6. Add deployment work independently where required: a durable production CAS
   backend and authenticated or signed substitute publication.
7. Revisit portable live action keys only after a real multi-repository live
   build sharing requirement exists.

## Acceptance Gates

| Workstream | Gate |
| --- | --- |
| Remote execution | Live ordinary package execution with declared logical mounts and a subsequent `100%` Foundry cache-hit rebuild are validated; automated coverage remains to be added |
| Identity correctness | Mutation tests demonstrate misses/new store paths for byte-affecting semantic changes, while import and collision failures remain enforced |
| External-cell integration | Foundry remotely builds against an immutable pinned BuckPkgs external-cell revision; substitute-only integration remains the preferred steady-state consumption model |
| Portable live cache keys, if triggered | Two independently rooted graphs obtain the same live action-cache identity only for genuinely identical canonical package actions |
