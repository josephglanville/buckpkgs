# Store Substitutes

## Goal

BuckPkgs needs byte-perfect substitutes for finalized store objects, especially
the bootstrap closure. The ordinary build graph should be able to import a known
good immutable store object instead of rebuilding the bootstrap tower.

The right comparison is Nix binary caches:

- package identity is known before execution
- a cache advertises a store object plus its references and archive digest
- the client verifies metadata and archive contents before trusting the import

BuckPkgs does not need to copy Nix's store implementation, but it should preserve
that separation between:

- **store-path identity**
- **archive transport identity**
- **realized tree content identity**

## Existing BuckPkgs Identities

The current model already distinguishes the right layers:

- `StorePathKey`
  - pre-build logical identity of `/pkgs/store/<key>-<name>`
- `OutputDigest`
  - CAS-visible identity of the realized tree
- package metadata
  - logical store path, runtime closure, runtime store outputs, package
    name/version/output

Substitution should add transport metadata without collapsing those layers.

## Recommended Format

Use a two-part format:

1. **Canonical tree payload**
   - NAR-compatible in spirit, and potentially NAR itself
   - serializes exactly the store object's filesystem tree
2. **BuckPkgs store-object manifest**
   - binds the payload to the BuckPkgs logical identity and closure semantics

### Why A NAR-Like Payload

The payload model should be intentionally small:

- directory
- regular file contents
- executable bit
- symlink target
- directory entries in deterministic lexical order

That matches the store-object semantics BuckPkgs needs and avoids tar-specific
ambiguities such as incidental uid/gid, owner names, pax headers, and unstable
header metadata.

The payload may literally be NAR, or a BuckPkgs-native equivalent with the same
object model. Reusing NAR would reduce format invention. A BuckPkgs-native format
would only be justified if direct CAS-tree import or streaming validation becomes
materially simpler.

The current implementation uses `buckpkgs-tree-v1`, a BuckPkgs-native,
uncompressed canonical payload. It records sorted directory traversal, regular
file bytes plus executable state, and symlink targets. This keeps the first
verified import path dependency-light while preserving an upgrade path to NAR
or compressed transport.

## What "Byte-Perfect" Means

The substitute payload must round-trip the logical store object exactly:

- file contents
- directory structure
- executable bit
- symlink targets

Those are the semantics that determine whether consumers observe the same store
object and whether Buck2 should accept the imported tree as the claimed artifact
value.

Host-local filesystem details should not become substitute identity by accident:

- uid/gid
- inode numbers
- ctime
- incidental extractor mtimes unless Buck2 deliberately makes them part of the
  store-object contract

If a local materializer preserves timestamps for replay fidelity, that should be
an explicit materialization policy. It should not silently turn wall-clock
metadata into part of the archive's logical identity.

## Store Object Manifest

Each substitutable output needs metadata at least like:

```json
{
  "format": "buckpkgs-store-object-v1",
  "store_path": "/pkgs/store/<key>-gcc-...",
  "store_path_key": "<key>",
  "store_entry": "<key>-gcc-...",
  "package": {
    "name": "gcc",
    "version": "...",
    "output": "out"
  },
  "archive": {
    "encoding": "buckpkgs-tree-v1",
    "compression": "none",
    "download_hash": "sha256:...",
    "download_size": 123456789,
    "payload_hash": "sha256:...",
    "payload_size": 123456789
  },
  "canonical_tree_hash": "sha256:...",
  "references": [
    "/pkgs/store/<dep>-glibc-...",
    "/pkgs/store/<dep>-binutils-..."
  ],
  "runtime_store_outputs": [
    "/pkgs/store/<dep>-glibc-...",
    "/pkgs/store/<dep>-gcc-..."
  ],
  "signatures": [
    "..."
  ]
}
```

The exact field names can evolve, but the semantic commitments should not:

- the manifest names the logical store object being substituted
- it commits to the byte payload
- it commits to the decoded canonical tree payload
- it names the closure edges required to consume the object
- it is signable as a stable document

## Closure Manifest

The bootstrap set should be published as a closure manifest, not as a loose bag
of independent archive URLs.

Conceptually:

```json
{
  "format": "buckpkgs-store-closure-v1",
  "name": "bootstrap-linux-x86_64",
  "roots": [
    "/pkgs/store/<key>-gcc-...",
    "/pkgs/store/<key>-binutils-...",
    "/pkgs/store/<key>-glibc-..."
  ],
  "objects": {
    "/pkgs/store/<key>-gcc-...": "store-object-manifest digest or URL",
    "/pkgs/store/<key>-binutils-...": "store-object-manifest digest or URL",
    "/pkgs/store/<key>-glibc-...": "store-object-manifest digest or URL"
  },
  "signatures": [
    "..."
  ]
}
```

This lets BuckPkgs:

- fetch just the closure entries needed by the requested toolchain/package
- verify that all declared references are present
- refuse partial or mismatched bootstrap bundles

## Trust Model

BuckPkgs store paths are input-addressed, not generally content-addressed.
Therefore:

- archive hashes prove bytes match the manifest
- tree digests prove decoded contents match the claimed Buck2 artifact value
- signatures or an equivalent trusted publication channel prove the manifest is
  acceptable for that input-addressed logical store path

Unsigned imports may be useful for local experiments, but the normal bootstrap
substitute path should require trust metadata.

## Implemented Object Pipeline

The repository now has an object-level substitution pipeline:

- `pkgs_export_store_substitute(...)` exports a live `PkgsPackageInfo` output
  into a `buckpkgs-tree-v1` payload plus `buckpkgs-store-object-v1` JSON manifest
- `pkgs_export_store_tree_substitute(...)` exports a declared tree with explicit
  store identity, which supports isolated import tests and externally prepared
  finalized trees
- `pkgs_imported_store_output(...)` verifies that payload and manifest, then
  returns the same `PkgsPackageInfo` surface backed by a store output
- `pkgs_prebuilt_store_substitute(...)` binds externally published archive and
  manifest artifacts to the import rule
- `pkgs_hydrate_store_object` verifies an object manifest and atomically
  publishes it below a selected store root, defaulting to `/pkgs/store`

The object manifest currently verifies store identity, package metadata, target
system, exact reference metadata, payload hash/size, and the decoded canonical
tree hash. Signature verification and closure-manifest orchestration remain to
be implemented before published bootstrap substitutes should be treated as
trusted ordinary-build inputs.

## Preferred Bootstrap-Island Import Pipeline

For the bootstrap island, the cleanest ordinary-build flow is deliberately
outside Buck2:

1. a dedicated bootstrap hydration command selects the named closure manifest
   for the host/target pair
2. that hydrator fetches canonical substitute blobs directly from the configured
   artifact service:
   - local archive cache
   - remote substitute service
   - remote-cache CAS used purely as byte transport
3. the hydrator verifies:
   - manifest signature/trust policy
   - logical store path identity
   - archive download hash/size
   - payload hash/size
   - decoded tree digest against `output_tree_digest`
   - referenced closure entries
4. the hydrator expands into temporary siblings and atomically publishes the
   finalized store objects at `/pkgs/store/...`
5. ordinary Buck graphs consume thin imported-provider declarations for those
   already-realized store objects and fail clearly if the required closure was
   not hydrated first

That keeps the bootstrap seed graph islanded. Buck can use the resulting store
objects as artifacts, but it does not participate in fetching or deriving them
on the ordinary build path.

## Recommended Buck2 Integration

The Buck2 fork already has most of the local path semantics:

- store-path declaration in Starlark
- staged output artifacts carrying logical store paths
- materializer-owned publication into `/pkgs/store`
- verification of existing store outputs against recorded artifact values
- atomic publication on first realization

The BuckPkgs prototype now implements archive import as a normal verified action
through `pkgs_imported_store_output(...)`. A native Buck2 store-import action is
still the appropriate later optimization if direct CAS tree import or stronger
diagnostic integration becomes valuable.

### Option A: Archive Import Action

The current BuckPkgs rule performs the equivalent operation:

```python
pkgs_imported_store_output(
    name = "gcc",
    substitute = ":gcc_substitute",
    runtime_inputs = [":glibc", ":binutils"],
)
```

Execution would:

1. verify manifest and archive metadata
2. decode the archive into the declared staged output path
3. fingerprint the staged tree into an `ArtifactValue`
4. rely on the existing store materializer to publish atomically

This is the simplest implementation path.

### Option B: Direct CAS Tree Import

If the substitute service can publish a Buck2-compatible CAS tree digest in
addition to the archive, Buck2 could:

1. verify the signed manifest
2. download or hydrate the CAS tree directly
3. reuse a `cas_artifact`-style action to obtain the output artifact value
4. materialize the logical store output without first unpacking archive bytes to
   a staged filesystem tree

This is the higher-performance path, but it requires stronger coupling to the
RE/CAS representation.

### Recommendation

For the bootstrap island, continue with closure publication and the external
hydration workflow on top of the implemented object pipeline:

- the manifest already carries the logical store path
- the manifest already carries the decoded tree digest
- archive transport and realized-tree identity stay separate
- ordinary Buck builds stay decoupled from substitute discovery and transport

Buck2-native import actions can remain a later integration path for workflows
that benefit from tighter action-graph coupling, but they should not be the
required mechanism for hydrating the bootstrap island.

## Optional Future Buck2 Hook Points

The first implementation can stay close to existing patterns in the fork:

1. **Starlark API**
   - add a sibling to `ctx.actions.cas_artifact(...)` near the current download
     action methods
   - conceptually:
     ```python
     ctx.actions.import_store_archive(
         output = store_output.as_output(),
         manifest = manifest_artifact,
         archive = archive_artifact,
     )
     ```
2. **Action implementation**
   - add an action beside the existing CAS-artifact action
   - parse and verify the manifest
   - decode the archive into the staged output path Buck2 already knows how to
     fingerprint
3. **Artifact handoff**
   - return an ordinary `ArtifactValue`
   - preserve the logical store-path association already carried by staged store
     outputs
4. **Materialization**
   - reuse the current store-output materializer path
   - do not let the import action write directly into `/pkgs/store`
5. **Diagnostics**
   - surface the logical store path, manifest identity, and failing verification
     stage in errors

That cut would keep substitution execution aligned with Buck2's existing
artifact model while avoiding a new direct-to-store mutation path. It is useful
for integrated import workflows, but the bootstrap island should not depend on
it.

## Relationship To Existing Buck2 APIs

The closest existing Buck2 primitive is `ctx.actions.cas_artifact(...)`, which
declares an artifact from a known CAS digest. Store substitutes need a sibling
primitive rather than a wrapper around ordinary shell actions because they need:

- first-class manifest validation
- stable error reporting for missing or untrusted substitutes
- a direct path to store-output artifact publication
- future CAS-tree short-circuiting

Likewise, existing store-output materialization should remain the only Buck2 code
that publishes into `/pkgs/store`. External bootstrap hydration is a separate
entry point with its own atomic publication logic; Buck2 import actions, if
added later, should still produce staged artifact values rather than directly
mutating the host store.

## Cache Lookup Policy

The importer needs a substitute index, but lookup policy should stay simple:

- key by exact logical store path or exact `StorePathKey`
- prefer local manifest/archive cache
- then consult configured remote substitute caches
- cache positive and negative lookup results locally
- do not silently substitute a different logical store path merely because bytes
  happen to match

Bootstrap consumers should fail if the exact expected store object is absent.

## Bootstrap Workflow

Publishing a bootstrap substitute closure should be explicit:

1. build the bootstrap island from source
2. verify reproducibility and foreign-seed independence
3. export every finalized store object into canonical archives
4. emit object manifests plus one closure manifest
5. sign the manifests
6. publish them to the configured substitute location

Ordinary builds then import from those manifests. They do not rebuild the island
as fallback.

## Open Implementation Questions

1. Use literal NAR payloads or a BuckPkgs-native byte-equivalent tree format?
2. Is archive compression fixed by policy or manifest-selectable?
3. Should tree-digest verification reuse Buck2 directory serializers directly,
   or reconstruct and then fingerprint through the existing materializer path?
4. How should the local substitute index be stored:
   - sqlite alongside materializer state
   - a separate BuckPkgs cache database
5. How do remote executors receive imported store closures:
   - pre-mounted from CAS
   - encoded as ordinary declared action inputs with store-path mapping

## Acceptance Criteria

Store substitution is real when:

1. a finalized bootstrap closure can be exported without losing tree fidelity
2. a clean machine can import that closure without building bootstrap packages
3. imported outputs materialize to the exact expected logical store paths
4. manifests reject mismatched archive bytes, mismatched tree digests, and
   wrong logical store paths
5. ordinary package/toolchain providers remain unchanged from the consumer's
   point of view
