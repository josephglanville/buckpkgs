# Buck2-Native Model

The Buck2-native version of BuckPkgs should use Buck2 for execution without forcing
the package universe to inherit Buck2's relocatable-artifact assumptions.

The split should be:

- `/pkgs/store/...` is the **logical package ABI**
- Buck2 actions, artifacts, RE, and CAS are the **physical realization layer**

That keeps the useful Nix property, stable absolute prefixes, while still making
package builds native Buck2 work.

## Buck2 Facts That Matter

Buck2 already separates three concerns:

1. **Graph identity**
   - `ActionKey` identifies an action inside the configured graph.
   - `BuildArtifact` ties an output path to the action that produces it.
2. **Execution-cache identity**
   - the RE action digest is built from the remote action payload: command,
     inputs, env, platform, and declared outputs
   - that digest is the cache key that matters for remote execution
3. **Artifact content**
   - artifact values carry file/tree digests
   - Buck2 can materialize them locally or ship them through CAS

BuckPkgs should reuse all three, but none of them should replace the package store
path.

## Recommended Core Model

### 1. A package instance is a configured Buck2 node

A resolved BuckPkgs package instance should behave like any other configured node in
Buck2:

- package identity is resolved during analysis from locked package-set data
- the node registers normal Buck2 actions for fetch, unpack, patch, configure,
  build, install, and fixup
- dependencies are normal artifact/provider dependencies
- the final install trees are ordinary Buck2 output artifacts

BuckPkgs should not introduce a parallel executor or scheduler.

### 2. Store paths are the package ABI

Each declared package output gets a pre-build-known absolute path such as:

```text
/pkgs/store/<key>-bash-5.3
/pkgs/store/<key>-bash-5.3-dev
```

Recipes are allowed to use those paths in:

- `configure --prefix`
- shebangs
- wrappers
- RPATHs
- pkg-config files
- generated scripts and metadata

This is not an accidental compatibility feature. It is how BuckPkgs avoids requiring
the whole Unix package corpus to become relocatable.

### 3. Buck2 artifacts realize store paths

The bytes for each store path should still be produced as ordinary Buck2
artifacts and backed by CAS tree digests.

That gives BuckPkgs:

- normal Buck2 scheduling
- normal RE action-cache hits
- normal CAS upload/download
- deferred local materialization

The store path is the logical name that recipes and dependents use. The CAS tree
digest is the realized content identity.

### 4. Consumers use providers that carry store semantics

Downstream Buck2 rules should consume providers rather than manually resolving
paths from package names.

The provider surface should include:

- `PkgsPackageInfo`
  - store paths by output name
  - output artifacts
  - declared runtime/build closures
  - metadata
- `RunInfo` for directly runnable tools
- a BuckPkgs environment provider for assembling `PATH`, library, include, and
  service views from store paths
- later, typed adapters for C/C++ toolchains, pkg-config data, interpreters, and
  similar integration points

The difference from a relocatable Buck2 tool is that those providers expose
stable absolute store paths as part of the contract.

### 5. Remote execution mounts only the needed closure

The store can be global on developer machines, but remote workers should not get
a global package universe.

For each action, BuckPkgs should compute the required store-path closure from
providers and mount only those paths into the remote input root. The action sees
the same absolute `/pkgs/store/...` paths that recipes embedded, but the worker
receives only the needed trees.

## Identity Split

BuckPkgs should keep these identities separate:

- `PackageInstanceDigest`
  - BuckPkgs' semantic identity for the resolved package instance
- `StorePathKey`
  - the pre-build-known identity of one output under `/pkgs/store`
- `ActionDigest`
  - Buck2/RE cache identity for one lowered action
- `OutputDigest`
  - CAS digest of the realized file tree

The store path key should be input-addressed and output-specific. It should not
be the action digest or output digest.

## Why Not Use Buck2 Content Paths As The Package Prefix

Content-addressed output paths are the wrong package ABI here:

1. many Unix packages need their final prefix before the build starts
2. the prefix itself is often embedded into output bytes
3. an output digest is only known after those bytes exist

That creates a cycle. Buck2 also intentionally writes content-based outputs
first at an `output_artifacts` placeholder before moving them to their final
content path, which is useful for Buck2 artifacts but not for a Unix package
prefix.

## Proposed First Slice

1. Resolve locked package data into a native BuckPkgs package node.
2. Compute stable store paths for `gnused`, `gnugrep`, and `gawk`.
3. Lower their builds to ordinary Buck2 actions.
4. Expose each package through:
   - store paths by output name
   - output artifact trees
   - `RunInfo`
   - one environment-fragment provider
5. Run an ordinary Buck2 action that receives only the required store-path
   closure and no host tools.

If that works, BuckPkgs has proven the intended model:

```text
locked package data
  -> BuckPkgs package/store identity
  -> ordinary Buck2 actions
  -> ordinary Buck2 artifacts
  -> RE action cache + CAS
  -> mounted /pkgs/store closure for consumers
```

## Staging Without A Buck2 Fork

If BuckPkgs store keys are derived entirely from BuckPkgs data, the first implementation
does not need Buck2 internals to compute package identity.

A practical non-fork prototype can be:

1. BuckPkgs resolves package definitions and computes store paths itself.
2. Package realization is represented as ordinary Buck2 graph nodes that build
   output trees under `buck-out`.
3. A BuckPkgs-side realization step materializes those trees into the local global
   `/pkgs/store`.

That is enough to validate:

- the manifest model
- the store-key algorithm
- the package graph
- builders
- nixpkgs porting
- bootstrap sequencing

Without later integration work, the first likely fork point is not package
realization. It is **transparent consumption**:

- stock Buck2 action outputs are project-relative, not arbitrary absolute paths
- stock Buck2 input roots are assembled from Buck2 artifacts under project
  relative paths
- an absolute symlink can point at `/pkgs/store/...`, but that only helps if the
  target already exists on the worker

So local development can use a pre-realized global store without a fork.

Remote execution is intentionally deferred for now, but the design should keep
the later path open:

- every consumed store path must correspond to a known artifact/tree digest
- closures must be explicit and computable
- package use must not depend on ambient undeclared host state
- it should be possible later to map a declared store-path closure onto remote
  worker mounts without changing package identity

That is enough to avoid painting BuckPkgs into a corner while keeping the current
design focused on local realization and package semantics.

This suggests a useful sequence:

1. prototype package semantics outside a Buck2 fork
2. prove that store keys and nixpkgs-style recipes work
3. keep closure metadata explicit so later RE mounting is straightforward
4. fork Buck2 only when adding first-class store-path semantics, automatic
   propagation, and cleaner provider-aware consumption
