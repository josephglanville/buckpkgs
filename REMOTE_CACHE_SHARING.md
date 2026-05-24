# Remote Execution And Cache Sharing

## Purpose

This document separates three related but distinct BuckPkgs/Buck2 workstreams:

1. remote-execution support for declared `/pkgs/store` inputs and outputs
2. cache and store identity correctness, including validation
3. cache-key portability across independent consuming repositories

The split matters because a monorepo commitment removes most of workstream 3,
but it does not remove workstreams 1 or 2.

## Decision Summary

The initial native BuckPkgs implementation may reasonably commit to a canonical
monorepo or canonical package-owning cell:

- package outputs have one authoritative configured-target identity
- developers and CI can share Foundry action-cache entries for that graph
- package actions do not initially need identical RE action digests across
  unrelated project roots or external-cell aliases

This is a meaningful simplification. It is not an alternative to implementing
remote store closures correctly.

Supporting BuckPkgs as an external cell in unrelated projects with build-once
package reuse is a larger product contract:

- the same resolved package output must retain the same logical store identity
- equivalent package executions must lower to the same RE action identity even
  when their consuming repository, cell alias, or owner paths differ
- promoted packages should have a trusted store-substitute publication path

The portable model should be pursued only if independent repositories are an
intended consumer boundary rather than an incidental development arrangement.

## Identities And Guarantees

BuckPkgs must keep four identities distinct:

| Identity | Meaning | Required property |
| --- | --- | --- |
| `PackageInstanceDigest` | Semantic resolved package instance | Changes for any declared byte-affecting package input |
| `StorePathKey` | Logical identity of one output under `/pkgs/store` | Safe immutable public package path |
| `ActionDigest` | Buck2/RE identity of a concrete execution | Commits to every execution-visible input and output contract |
| `OutputDigest` | CAS identity of realized bytes | Deduplicates identical realized content |

The desired shared-cache behavior has two possible forms:

```text
same ActionDigest -> Foundry action-cache hit -> reuse realized CAS output
```

or, for explicitly published objects:

```text
same StorePathKey -> trusted substitute manifest -> import realized CAS output
```

CAS storage alone deduplicates transferred or retained bytes. It does not mean
that a package action executes only once unless an action-cache hit or a trusted
substitute lookup avoids execution.

## Current Implementation Status

BuckPkgs already derives logical output paths from semantic package inputs in
`rules/pkgs.bzl` and documents the intended contract in `STORE_PATHS.md`.
Native store outputs and CAS store imports are present in the Buck2 fork.

The current RE implementation is not yet sufficient for package DAG execution:

- `../buck2/app/buck2_execute_impl/src/executors/re.rs` rejects remote actions
  with non-empty declared BuckPkgs store-input closures.
- `../buck2/app/buck2_execute_impl/src/executors/hybrid.rs` forces those
  actions to local execution in the OSS hybrid executor.
- `../buck2/app/buck2_execute/src/execute/request.rs` represents each store
  input with both its logical store path and its staged artifact path.
- `../buck2/app/buck2_execute/src/execute/command_executor.rs` includes the
  staged path in the store-closure salt used for RE action identity.
- `../buck2/app/buck2_build_api/src/interpreter/rule_defs/cmd_args/builder.rs`
  renders input store artifacts as `/pkgs/store/...`, but renders store outputs
  as executor staging paths.

Consequently, normal package actions consuming store dependencies cannot yet
execute remotely, and actions that are semantically the same package build may
have different action keys when owned by different project graphs.

Foundry already has the relevant generic mechanisms:

- its action cache reuses successful cacheable executions by RE action digest
- its Linux sandbox executor can mount materialized directories at fixed
  absolute targets

The missing portion is the BuckPkgs-specific declared-store protocol between
Buck2 action lowering and Foundry sandbox setup.

## Workstream 1: Remote Execution Plumbing

**Required for:** monorepo and portable external-cell use.

**Purpose:** allow a package action consuming declared store outputs to execute
remotely with the same absolute `/pkgs/store/...` view it has locally.

This work is not fundamentally about cache hit rate. Without it, meaningful
package dependency graphs remain local-only.

### Required Changes

1. Carry declared store closure inputs through the RE action request in a form
   Foundry can materialize from CAS.
2. Expose only the declared closure read-only at `/pkgs/store` in the remote
   sandbox.
3. Reject arbitrary absolute mount targets; the protocol should authorize only
   the versioned BuckPkgs store mount.
4. Collect package outputs through executor-owned staged outputs, then publish
   them through Buck2's store materializer rather than permitting actions to
   mutate the host store.
5. Support action-cache downloads and remote execution results for store
   outputs using the same materialization path.

### Protocol Direction

Even for a monorepo, a reserved RE subtree gives the protocol a clear security
and debugging boundary:

```text
RE input root:
  __buckpkgs__/store/<store-entry>/...

remote sandbox view:
  /pkgs/store/<store-entry>/...
```

A versioned action property or canonical manifest can declare that
`__buckpkgs__/store` is the only input subtree permitted to mount at
`/pkgs/store`.

For a monorepo-only implementation, that subtree does not need to eliminate
every owner-specific path elsewhere in the action. It only needs to represent
the declared store inputs faithfully and participate in action identity.

### Completion Criteria

- a package action with at least one store dependency executes on Foundry
- the action observes only its declared `/pkgs/store` closure
- undeclared store paths are unavailable in the sandbox
- a remote result materializes as the expected immutable local store output
- repeated execution receives action-cache hits under stable monorepo inputs

## Workstream 2: Cache Identity Correctness And Validation

**Required for:** monorepo and portable external-cell use.

**Purpose:** make both logical immutable store paths and RE cache hits sound.

This is correctness work. It must not be deferred on the theory that cache
sharing is only a performance feature.

### Store Identity Correctness

`StorePathKey` must change whenever bytes at the published logical store path
may change. Its canonical semantic descriptor must commit to at least:

- store ABI version
- structured builder kind and builder behavior version
- actual builder/tool implementation identity when it can affect output bytes
- normalized recipe arguments, environment, hooks and fixups
- fixed-output source identities and patch identities
- build, host and target platform semantics
- output name and visible store name
- direct dependency logical store paths, grouped by role

The current package identity includes `ctx.attrs.builder` and
`STORE_ABI_VERSION`. The native design must ensure that a behavior-changing
builder or rules implementation change cannot silently retain the same logical
store path merely because its symbolic builder name remained the same.

### Action Cache Correctness

`ActionDigest` must commit to every execution-visible fact that may affect
bytes, including:

- command line and environment
- tools and ordinary input artifacts
- platform and sandbox behavior
- the declared store closure bytes and their `/pkgs/store/...` visibility
- the output-path contract observed by the package command

This does not require a portable action digest. A monorepo action digest may
contain stable monorepo-specific paths and still be correct. It becomes wrong
only if byte-affecting inputs or execution semantics are omitted.

### Materialization And Substitute Correctness

The store is input-addressed, so an existing or downloaded tree cannot be
trusted merely because it occupies the requested path.

The materializer and substitute importer must:

- verify an existing store object against the expected artifact value or
  authenticated manifest
- reject collisions where one `StorePathKey` resolves to differing bytes
- seal or otherwise preserve output immutability
- keep substitute publication authenticated for non-local trust domains

`STORE_SUBSTITUTES.md` owns the detailed substitute format and trust model.
Substitution complements action caching; it must not cause the RE action cache
to be keyed directly by `StorePathKey`.

### Validation Matrix

The following checks are required regardless of repository topology:

| Change | Expected result |
| --- | --- |
| Source or patch bytes change | New `StorePathKey`; no reuse of old store object |
| Dependency store path changes | New `StorePathKey` |
| Target platform semantics change | New `StorePathKey` |
| Builder/tool behavior ABI changes | New `StorePathKey` |
| Declared store dependency bytes change | Different `ActionDigest` |
| Undeclared store content exists on worker | No effect on build; inaccessible |
| Existing store path contains wrong tree | Verification failure, not silent reuse |
| Imported manifest has wrong tree/reference metadata | Import failure |

## Workstream 3: Cache-Key Portability

**Required for:** build-once reuse across unrelated projects consuming BuckPkgs
as an external cell.

**Not required for:** a canonical monorepo/package-owning-cell contract.

**Purpose:** prevent irrelevant integration details from turning a semantically
identical package build into a different RE action-cache entry.

Without this work, two projects may derive the same:

```text
/pkgs/store/<StorePathKey>-<name>
```

while still missing each other's Foundry action-cache entries because their
actions mention different staged paths, configured owners, cell aliases, tools
or execution configurations. That is normally a false miss, not an unsound hit.

### Required Changes For Portable Build Reuse

1. Remove consumer-specific staged store-input paths from the RE identity of
   package actions. The declared store entry and its bytes are the relevant
   inputs.
2. Give package store outputs a canonical executor-visible output path such as:

```text
__buckpkgs__/out/<store-entry>
```

   This is necessary because packages can embed build and output paths in their
   bytes; deleting a path from a hash without canonicalizing what execution
   observes would be unsound.
3. Canonicalize source, patch and builder-tool paths used by package-producing
   actions when those paths otherwise derive from consumer ownership.
4. Define a dedicated BuckPkgs package execution platform so consumer roots
   cannot incidentally change environment, platform properties or tool choice.
5. Decide whether tool executables become store-addressed inputs or enter the
   package RE protocol under another stable content-derived path.
6. Require projects intending to share live build cache entries to use the same
   Foundry cache namespace and compatible trust and execution policy.

### Portable Completion Test

Use two independent repositories with:

- distinct roots and empty local stores
- different BuckPkgs external-cell aliases
- the same pinned BuckPkgs package-set revision
- the same Foundry AC/CAS namespace

Build a package that consumes at least one store dependency.

The test passes only when:

- both projects calculate the same logical store path
- both lower the package action to the same RE `ActionDigest`
- project A executes and populates Foundry
- project B obtains action-cache hits without executing the package DAG
- changing recipe semantics, builder ABI, dependency store identity or platform
  causes a store-key change or cache miss as appropriate

## Monorepo Versus Portable Consumption

| Capability | Canonical monorepo | Independent external-cell consumers |
| --- | --- | --- |
| Correct immutable store identity | Required | Required |
| Remote store closure execution | Required | Required |
| Correct remote action hashing | Required | Required |
| Stable cache reuse within CI/developer graph | Required | Required |
| Independence from cell aliases and repository ownership | Not required | Required |
| Canonical package-action path protocol beyond store closure mounts | Optional initially | Required |
| Trusted published substitutes | Useful for bootstrap/releases | Strongly desirable |

A monorepo contract therefore removes the broad action-canonicalization and
cross-consumer validation problem. It still requires a correct store ABI,
materializer behavior, and remote declared-store execution protocol.

## Recommended Implementation Order

1. Complete semantic store identity and its mutation tests, including
   builder/tool behavior versioning.
2. Implement and validate native local store materialization and collision
   handling.
3. Add the declared-store RE protocol and Foundry read-only `/pkgs/store`
   sandbox mounts.
4. Prove remote package DAG execution and action-cache reuse for the canonical
   monorepo/package-owning-cell case.
5. Keep reviewed CAS substitute import for bootstrap and explicitly promoted
   store objects.
6. Only after independent repository consumption is a committed requirement,
   implement canonical package action lowering and the two-root portability
   tests.

This ordering keeps correctness and useful RE capability on the critical path,
while treating cross-repository cache-key portability as an explicit product
choice rather than an accidental dependency of the initial fork.
