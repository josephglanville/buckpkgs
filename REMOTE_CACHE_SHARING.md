# Remote Execution And Cache Sharing

## Scope And Decision

BuckPkgs and Foundry remain separate repositories. Foundry's intended integration
boundary is a pinned BuckPkgs external cell at an immutable, exportable revision,
not a merged source tree.

That boundary gives consumers one canonical live-build identity when they use
the same cell name (`buckpkgs`), immutable Git revision, execution platform,
and RE namespace. Published, reviewed BuckPkgs store content imported from
Foundry CAS remains the preferred reuse boundary for finalized packages.

Keep three workstreams separate:

1. Remote execution support for actions that consume declared BuckPkgs store
   inputs.
2. Store/cache identity correctness and validation.
3. Portable live action-cache keys across independent Buck2 graphs.

Workstreams 1 and 2 are needed for reliable external-cell integration.
Workstream 3 has two materially different cases: pinned external consumers
already share live keys under the canonical boundary above; making a mutable
native checkout share those keys is optional work gated by source-identity
validation.

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
| End-to-end remotely executed ordinary package with store dependencies | Validated locally | On May 24, 2026, native BuckPkgs and Foundry's pinned `buckpkgs` external cell each remotely built `development/libraries/zlib:lib`; a subsequent external consumer completed with `148/148` Foundry cache hits |
| Live keys across pinned external consumers | Validated locally | A disposable second consumer using the canonical `buckpkgs` alias and Git revision `3724bca599521a08884e27d46ad772c7c9c715d6` reused the Foundry external-cell build without executing a remote command |
| Live keys between a native checkout and an external revision | Correctly not shared | After canonicalizing the native cell name, native `zlib` lowered to `6614ddc9...:175` and the pinned external-cell action lowered to `7f3758cf...:175`; the source-built Rust helper outputs also differed because source layout remains action-observable |
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
3. The native repository was then given its canonical self alias,
   `buckpkgs = .`, and both builds selected `buckpkgs//platforms:remote`.
   This removed the avoidable `root//` versus `buckpkgs//` and staged-store
   owner differences, but did not make a mutable checkout equivalent to a
   pinned external revision.
4. With canonical labels aligned, native `zlib` used action digest
   `6614ddc97a3def0dc2a10fa71a28f00d4dc7428c521ee803cdc41d02561edbfc:175`;
   Foundry's external-cell build used
   `7f3758cfc7a4df5a7d92ca4f3d79ecdb7e989fad03474a3583d920383f706e15:175`.
5. The difference is source-observable, not a Foundry cache failure. The
   native `pkgs_configure_make_install` Rust helper output digest was
   `bb9bfbf3...` while the external-cell helper output digest was `76accf8b...`.
   Native source artifacts execute from the checkout; Git external-cell
   sources execute from `buck-out/v2/external_cells/git/<commit>/...`.
6. A second disposable external consumer with the same `buckpkgs` alias,
   revision, platform, and Foundry RE namespace then built
   `buckpkgs//development/libraries/zlib:lib` with `100%` cache hits:
   `Commands: 148 (cached: 148, remote: 0, local: 0)`.

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

### Already Portable: Pinned External Consumers

Two consumer repositories that both declare the cell as `buckpkgs`, pin the
same immutable Git revision, select the same execution platform, and use the
same Foundry RE namespace share live package action-cache entries. Buck2's Git
external-cell source location is commit-qualified and consumer-root
independent, so this case does not require a new portable-key mechanism.

The canonical cell name matters. A consumer that exposes the same repository
under a different cell name is asking Buck2 to analyze a different configured
graph and is not part of this sharing contract.

### Correctly Not Portable: Mutable Native Checkout

The direct BuckPkgs checkout is not equivalent to an external Git pin merely
because its current files happen to match. It may have uncommitted edits, or
it may advance without changing the consumer pin. Reusing the external
revision's cache identity without validating that claim would be a correctness
bug.

Adding `buckpkgs = .` to the native configuration is still useful: it
canonicalizes target and staged-output ownership and removes avoidable identity
noise. It intentionally does not claim that checkout source artifacts are the
Git revision's source artifacts.

### Native-Equivalence Trigger And Design

Implement native-to-external live-cache sharing only if development workflows
need a clean native checkout to reuse unpublished external-cell build results.
That work must include:

1. An explicit native-cell revision claim tied to the external Git identity,
   with validation that every action-visible source is from that revision and
   that local source modifications cannot obtain a hit.
2. A logical source execution namespace for claimed cells. Commands and RE
   input trees must observe the same commit-qualified paths whether bytes came
   from a checkout or Buck2's external-cell materialization.
3. Canonical treatment of remaining action-observable store inputs, tools,
   environment entries, outputs, and platform properties. The staged-store
   transport path may be removed from action identity only once the executor
   consumes the canonical mapping without changing behavior.
4. Positive validation for a clean claimed native revision and the matching
   external cell, plus negative validation for dirty files, different
   revisions, changed dependencies, and different semantic configuration.

## Revised Sequence

1. Keep reviewed CAS substitutes and immutable store imports as the
   cross-repository distribution boundary.
2. Keep `platforms//:remote` and the Foundry v5 runtime profile validation
   path working, and automate both remote execution and the two-consumer
   external-cell cache-hit proof.
3. Require consumers that want live cross-repository reuse to use the
   canonical `buckpkgs` cell name and identical immutable revision pin.
4. Validate a representative ordinary package stack remotely once the `zlib`
   proof is covered in automation.
5. Close the remaining identity-policy gap with builder/ABI enforcement and
   mutation tests; require these checks for byte-affecting packaging changes.
6. Publish or identify an immutable exportable BuckPkgs revision and let
   Foundry consume it as a pinned external cell using published substitutes,
   without depending on a live local BuckPkgs checkout.
7. Add deployment work independently where required: a durable production CAS
   backend and authenticated or signed substitute publication.
8. Implement validated native-to-external key equivalence only if native
   development builds need that optimization.

## Acceptance Gates

| Workstream | Gate |
| --- | --- |
| Remote execution | Live ordinary package execution with declared logical mounts and a subsequent `100%` Foundry cache-hit rebuild are validated; automated coverage remains to be added |
| Identity correctness | Mutation tests demonstrate misses/new store paths for byte-affecting semantic changes, while import and collision failures remain enforced |
| External-cell integration | Foundry remotely builds against an immutable pinned BuckPkgs external-cell revision, and a second canonical external consumer retrieves its live actions from the same Foundry cache |
| Native-to-external key equivalence, if triggered | A revision-validated clean checkout obtains the pinned external action identity; dirty or semantically changed inputs demonstrably miss |
