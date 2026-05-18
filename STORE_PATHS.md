# Store Paths

`/pkgs/store/<pkgs_path>` is part of the BuckPkgs package ABI.

The useful Nix lesson is not just immutability. It is that ordinary Unix
packages can be built against a stable absolute prefix, and years of nixpkgs
recipes already solve for that environment. Requiring all package outputs to be
fully relocatable would discard that advantage.

The remaining question is what `<pkgs_path>` should be keyed by.

## Required Properties

The key must be:

1. known before the build starts
2. stable for a fully resolved package output
3. different whenever any declared input that can affect the build changes
4. output-specific, because multi-output packages have distinct absolute paths
5. independent from the bytes produced after the build

That rules out output-tree CAS digests as the normal key.

## Candidate Keys

### A. Output Digest

Example:

```text
/pkgs/store/<output_digest>-bash-5.3
```

This is attractive for deduplication, but it is wrong for the general case:

- the digest is not known before build execution
- packages cannot use their final prefix while configuring or building
- any package that embeds its own path creates a digest cycle

Use this only for fixed-output objects whose content digest is already declared,
such as fetched source archives.

### B. Buck2 Action Digest

Example:

```text
/pkgs/store/<action_digest>-bash-5.3
```

This is also the wrong layer:

- a package lowers to multiple Buck2 actions, not one
- action digests include declared output paths, so using the action digest to
  derive those same paths creates a cycle
- it couples the package ABI to executor details that should remain below the
  package model

Action digests should remain Buck2's execution-cache keys.

### C. Fully Resolved Package-Output Descriptor

Example:

```text
/pkgs/store/<store_path_key>-bash-5.3
/pkgs/store/<store_path_key>-bash-5.3-dev
```

This is the right default.

The store-path key should be a BuckPkgs-defined digest over a canonical descriptor
for one resolved package output.

## Recommendation

Use an **input-addressed package-output key**.

Conceptually:

```text
package_instance_digest = hash(
  store_abi_version,
  canonical resolved package definition,
  builder identity,
  fixed-output source digests,
  patch and hook digests,
  declared environment inputs,
  build/host/target platforms,
  declared output set,
  direct dependency store paths grouped by role
)

store_path_key(output) = hash(
  store_abi_version,
  package_instance_digest,
  output_name,
  store_name
)

store_path(output) =
  /pkgs/store/<store_path_key>-<store_name>
```

`store_name` should include the human-facing package name/version and the output
suffix when the output is not the default output.

### Why Direct Dependency Store Paths

Hash direct dependency **store paths**, not only package names or raw source
hashes:

- those are the actual absolute inputs visible to the build
- they recursively commit to each dependency's own declared inputs
- partitioning by role preserves the build/host/target distinction
- a dependency rename that changes its absolute path can change output bytes, so
  the visible path must be part of the key

The full transitive closure does not need to be duplicated in the descriptor if
direct dependency paths already commit to it transitively.

### Why `store_name` Must Affect The Key

The readable suffix is not merely cosmetic. Packages may embed their full own
prefix, including the suffix. Therefore:

- `bash-5.3`
- `bash-5.3-dev`
- any future rename

can change output bytes. The suffix must participate in the digest input, even
though it is also printed outside the hash.

### Why This Is Nix-Like But Not A Nix Derivation Store

This is the same useful category as classic Nix input-addressed store paths:

- path known before build
- path commits to declared inputs
- package can embed its own prefix

But BuckPkgs does not need `.drv` files as the execution substrate. The descriptor is
strict BuckPkgs data used to compute identity; Buck2 still performs execution, action
caching, CAS upload/download, and materialization.

## Fixed-Output Exception

For fixed-output fetches, the expected content digest is already known before
execution. Those can use a separate fixed-output key derived from:

- store ABI version
- declared content digest
- output name
- store name

That preserves mirror equivalence and avoids rebuilding identity around URLs or
fetch commands.

The fetching mechanism should still be ordinary Buck2 machinery such as
`http_file`, not a BuckPkgs-specific downloader. BuckPkgs only needs access to the
declared fixed-output digest at analysis time so that the package descriptor can
commit to content rather than URL spelling. If ordinary Buck2 fetch targets do
not expose that metadata directly, a thin provider layer is preferable to
reimplementing fetch rules inside BuckPkgs.

## Identities To Keep Distinct

- `PackageInstanceDigest`
  - semantic identity of the resolved package instance
- `StorePathKey`
  - logical identity of one output under `/pkgs/store`
- `ActionDigest`
  - Buck2/RE execution-cache key for a concrete action
- `OutputDigest`
  - CAS digest of the realized output tree

Only `StorePathKey` belongs in the visible store path. Only `OutputDigest`
identifies realized bytes.

## Open Follow-Ups

1. Choose the store ABI encoding:
   - digest algorithm
   - textual alphabet
   - visible hash length
2. Decide the canonical `store_name` format:
   - `name-version`
   - `name-version-output`
   - how to handle variants
3. Decide whether package-set provenance belongs only in lock metadata or also
   in the semantic descriptor. The default recommendation is metadata only:
   equal resolved packages should reuse the same store paths.
