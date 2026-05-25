# Buck2 Store Integration

Package output and payload policy is defined in
[PACKAGING.md](./PACKAGING.md). This document discusses only how Buck2 should
represent the resulting stable store outputs.

Goal:

```text
//pkgs/tools/compression/bzip2:bin
```

should be an ordinary Buck2 graph node that can be depended on like any other
target, but whose declared package outputs are realized at stable absolute paths
such as:

```text
/pkgs/store/<key>-bzip2-1.0.8
```

The cleanest design is to add a new **artifact path space** to Buck2, not a new
graph engine.

## Current Buck2 Shape

Today Buck2 mostly assumes:

- build outputs are `BuildArtifactPath`
- `BuildArtifactPath` resolves through `BuckOutPathResolver`
- resolved build outputs are `ProjectRelativePathBuf`
- materializer APIs accept `ProjectRelativePathBuf`
- action outputs reported to local and remote executors are project-relative

That means a normal build artifact can end up under `buck-out/...`, but not at an
absolute `/pkgs/store/...` path.

## Proposed New Concept

Add a first-class `StoreArtifactPath`.

Conceptually:

```rust
enum BuildOutputPath {
    BuckOut(BuildArtifactPath),
    Store(StoreArtifactPath),
}

struct StoreArtifactPath {
    owner: DeferredHolderKey,
    store_path: PkgsStorePath, // /pkgs/store/<key>-<name>
    projected_path: ForwardRelativePathBuf,
}
```

The exact names may differ. The important part is that Buck2 gains an artifact
path variant whose final materialization root is not under the project root.

`BuildArtifact` should then contain the generalized output path rather than only
`BuildArtifactPath`.

## Execution Model

Do **not** make normal local or remote executors write directly to
`/pkgs/store/...`.

Instead:

1. action analysis declares a store artifact as the logical output
2. Buck2 derives an internal project-relative staging path for execution
3. the action writes to the staging path exactly like any other Buck2 output
4. Buck2 hashes the produced tree into an `ArtifactValue`
5. the materializer declares the logical final path at `/pkgs/store/...`
6. materialization copies/links/downloads the realized tree into the final store
   location when needed

This is close to how content-based outputs already use an execution-time
placeholder path and a final path after output hashing. The difference is that
BuckPkgs final paths are pre-build-known and absolute.

## Why Staging Is Better Than Direct Absolute Outputs

- local and remote action protocols can stay project-relative
- output cleanup remains scoped to staging paths
- RE can stay deferred without blocking the first local implementation
- `/pkgs/store` stays immutable and materializer-owned
- existing action result parsing continues to work on normal relative paths

## Starlark Surface

Add a special output declaration only BuckPkgs rules need, for example:

```python
out = ctx.actions.declare_store_output(
    store_path = "/pkgs/store/<key>-bzip2-1.0.8",
    dir = True,
)
```

`pkgs_package(...)` would use that internally. Ordinary users should mostly see:

```python
pkgs_package(
    name = "bz2",
    ...
)
```

and consume:

```python
deps = ["//pkgs/tools/compression/bzip2:bin"]
```

The provider for the target should expose:

- logical store paths by output name
- the corresponding Buck2 artifact handles
- closure metadata

The final user-facing target still behaves like a normal dependency. It is
special only in the output-path class of its artifacts.

## Main Buck2 Touch Points

### 1. Artifact path model

Current likely touch points:

- `buck2_core::fs::buck_out_path::BuildArtifactPath`
- `buck2_artifact::artifact::build_artifact::BuildArtifact`
- `buck2_execute::path::artifact_path::ArtifactPath`
- `buck2_core::fs::artifact_path_resolver::ArtifactFs`

Needed change:

- generalize from "all built artifacts live in buck-out" to "built artifacts
  have a path kind, one of which is a store path"

### 2. Output declaration

Current likely touch points:

- `AnalysisRegistry::declare_output`
- `ActionsRegistry::declare_artifact`
- `ctx.actions.declare_output`

Needed change:

- add a store-output declaration path that claims store paths separately from
  ordinary target-relative output paths
- still binds the output to an `ActionKey` like any other artifact

### 3. Path resolution

Current likely touch points:

- `ArtifactFs::resolve_build`
- `ArtifactPath::resolve`

Needed change:

- distinguish:
  - execution path: project-relative staging path
  - logical/final path: absolute `/pkgs/store/...`
- command-line formatting for a store artifact should generally expose the
  logical store path, because recipes need to embed it
- executor bookkeeping should use the staging path

This probably means one resolver is not enough anymore. Buck2 needs explicit
methods such as:

```text
resolve_for_execution(...)
resolve_for_materialization(...)
resolve_for_command_line(...)
```

instead of assuming one path serves every purpose.

### 4. Materialization

Current likely touch points:

- `Materializer`
- `DeclareArtifactPayload`
- requested-artifact materialization

Needed change:

- support materializer keys that are not only `ProjectRelativePathBuf`
- or add a dedicated store-materializer side table keyed by absolute store paths
- materialization of a store artifact should be idempotent and immutable

This is the first genuinely invasive area. Today the materializer is strongly
typed around project-relative paths.

### 5. Action execution bookkeeping

Current likely touch points:

- `CommandExecutionOutput`
- `ResolvedCommandExecutionOutput`
- local executor output extraction
- RE download/output mapping

Needed change:

- executors continue to receive only staging outputs
- store artifacts map from logical output -> staging output -> `ArtifactValue`
- after execution, declaration happens at the logical store path

This keeps local and remote protocols mostly unchanged.

## What Should Stay Unchanged

- configured target identity
- `ActionKey`
- action registration
- RE action digest construction
- dependency edges
- providers
- CAS `ArtifactValue` handling

`//pkgs/tools/compression/bzip2:bin` should look like an ordinary configured
target whose
actions produce ordinary `ArtifactValue`s. Only the final output namespace is
special.

## Open Design Choice

There are two plausible ways to represent path generalization internally:

### Option A: Generalize the existing path types

```text
BuildArtifactPath -> enum of BuckOutPath | StorePath
Resolved path -> enum of ProjectRelative | AbsoluteStore
```

Pros:

- conceptually honest
- store outputs become fully first-class everywhere

Cons:

- touches many APIs that currently assume `ProjectRelativePathBuf`

### Option B: Keep build artifacts project-relative, add store aliases

```text
BuildArtifactPath stays buck-out
StoreArtifactInfo maps store path -> artifact
Materializer knows aliases
```

Pros:

- smaller initial patch
- less churn through executor code

Cons:

- store paths are not truly artifact paths
- command-line/path formatting becomes special-case plumbing
- easier to drift into "side effect plus metadata" rather than first-class
  outputs

## Recommendation

For the intended end state, choose **Option A**.

If `/pkgs/store` is part of the package ABI, the final system should model store
outputs as real artifact paths. Hiding them behind aliases would reduce the
first patch but keep the core impedance mismatch alive.

The implementation can still stage execution outputs project-relatively; that is
an execution detail, not a reason to demote store outputs from the artifact
model.
