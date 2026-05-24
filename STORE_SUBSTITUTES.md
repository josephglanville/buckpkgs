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

## Implemented Pipeline

The repository now has object and bootstrap-closure substitution pipelines:

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
- `pkgs_export_store_closure(...)` exports a complete named closure bundle from
  object substitutes and refuses incomplete or unreachable object sets
- `pkgs_hydrate_store_closure` verifies a pinned closure plus every bundled
  object before atomically hydrating store objects below `/pkgs/store`
- `pkgs_hydrated_store_output(...)` passes the reviewed manifest source and its
  declared provider contract directly to Buck2's native store-import action,
  which authenticates the manifest and canonical tree hash while constructing
  its `ArtifactValue` in one physical traversal, without depending on the live
  producer graph or creating a staged copy
- `pkgs_cas_store_output(...)` passes a reviewed CAS publication manifest to
  Buck2's CAS store-import action, which fetches a pinned REAPI directory graph
  from Foundry and atomically publishes the same logical store output without
  requiring an already-hydrated physical tree

The manifests verify store identity, package metadata, target system, exact
canonical reference metadata, payload hash/size, decoded canonical tree hash,
closure completeness, and reachability from the named roots. For the current
bootstrap prototype, repository-pinned JSON under
`bootstrap/substitutes/linux_x86_64/` is the trusted publication metadata.
Cryptographic signatures or a separately authenticated publication channel are
still required before consuming bundles obtained from an untrusted service.

## CAS Publication Overlay

CAS publication is an additive transport representation of an approved store
object. A CAS-capable manifest retains the canonical archive/tree identity and
adds:

```json
{
  "cas": {
    "format": "reapi-directory-v1",
    "digest_function": "sha256",
    "root_digest": "<sha256-hex>:<encoded-directory-size>"
  }
}
```

`foundryctl cas upload-tree /pkgs/store/<entry>` uploads file, directory, and
symlink nodes and returns this root digest. All 24 role-specific objects in the
current normalized bootstrap substitute closure have parallel
`.cas.manifest.json` overlays, so the archive closure manifest and hydration
bundle remain valid while normal consumers use the CAS representation.

After upload, `pkgs_add_cas_manifest` accepts the exported store-object
manifest and returned root digest, validates both, and produces the
`.cas.manifest.json` overlay plus a Starlark pin record containing
`canonical_tree_hash` and `cas_root_digest`. `BUCK` files load that generated
record when declaring `pkgs_cas_store_output(...)`; publication review compares
the checked-in files byte-for-byte with regenerated output rather than
transcribing digests by hand.

An REAPI directory commits to contents, executable bits, and symlink targets,
but not BuckPkgs' sealed write bits or normalized mtimes. The Buck2 CAS
store-import materializer therefore normalizes timestamps and seals fetched
trees before atomic `/pkgs/store/...` publication. Native package outputs still
must arrive sealed and are rejected if writable.

For a previously published physical tree, the CAS importer first authenticates
the existing bytes against the pinned canonical tree. Its first CAS-backed use
normalizes and seals a valid legacy tree; subsequent fresh Buck2 daemons verify
and reuse that sealed `/pkgs/store/...` object without fetching the CAS payload
again. The retained CAS remains the substitution source when the local store
object is absent.

## Preferred Bootstrap-Island Import Pipeline

For the bootstrap island, the ordinary-build flow is deliberately split around
Buck2:

1. an explicit publication workflow exports finalized roots; after a reviewed
   baseline exists, selected repaired outputs may be produced from imported
   published inputs and assembled with unchanged archived objects without
   selecting the expensive live closure bundle target
2. the pinned `bootstrap/substitutes/linux_x86_64/closure.json` selects and
   authenticates the expected named closure for the host/target pair through
   source review and version control
3. `pkgs_hydrate_store_closure` consumes that pinned closure and a corresponding
   bundle fetched from the configured artifact service:
   - local archive cache
   - remote substitute service
   - remote-cache CAS used purely as byte transport
4. the hydrator verifies:
   - bundle contents against the repository-pinned closure metadata
   - logical store path identity
   - archive download hash/size
   - payload hash/size
   - decoded canonical tree digest
   - referenced closure entries
5. the hydrator expands into temporary siblings and atomically publishes the
   finalized store objects at `/pkgs/store/...`
6. ordinary Buck graphs use `pkgs_hydrated_store_output(...)` to pass each
   pinned manifest into Buck2's native import action, which verifies the
   declared provider metadata and already-realized store object before
   registering it as a store output, failing clearly if hydration has not run

The native Buck2 import action reads the reviewed source manifest directly,
validates its package/store/runtime contract, hashes the physical
`/pkgs/store/...` tree in canonical BuckPkgs order while constructing Buck2's
`ArtifactValue`, and registers that value directly with the store materializer
without projecting bytes through a staged output. Imported store outputs use
content-addressed staged identities and a source manifest input. Native package
build tools are execution dependencies, so ordinary packages and package-backed
toolchains share the imported tool action nodes in the usual host-build case.
This keeps the bootstrap seed graph islanded and does not make ordinary builds
fetch or derive the live bootstrap tower.

The pinned normalized bootstrap closure contains role-specific GMP, MPFR,
libmpc, glibc, Linux-header, GCC, and Binutils objects plus Bash, GNU Make,
Coreutils, Findutils, GNU sed, GNU grep, GNU awk, and GNU patch. This is
sufficient for `development/libraries/zlib:lib`, the reduced Python/Meson
path, Bubblewrap, and PostgreSQL to build as ordinary imported-bootstrap
packages. Ordinary package definitions depend on canonical labels such as
`development/libraries/glibc:lib`, `development/libraries/glibc:dev`,
`development/compilers/gcc:{bin,dev,libgcc,libstdcxx}`, and
`tools/text/gawk:bin`; only those aliases may consume the reviewed
`bootstrap/substitutes` transport layer. Live publication producers are
restricted to bootstrap producers, exports, and validation targets through an
explicit visibility allowlist. Producer-side seed checks and individual export
actions are similarly internal-only dependency surfaces, so they cannot act as
bridges back into the live island.
Pinned manifests authenticate a canonical BuckPkgs payload hash, while Buck2
requires its own RE-directory-shaped artifact fingerprint. The native import
action now first authenticates the manifest contract itself, then derives both
tree identities from the same byte stream: Buck2 reads the imported object
once to validate the canonical hash and construct its artifact value. This
eliminates the previous verifier-plus-fingerprint duplicate read in addition
to the staged copy and materializer re-verification. A future manifest field
containing compatible Buck2 directory digest metadata for the configured
digest algorithms, or a durable trusted receipt, could avoid or reuse that
remaining single import traversal. This cost is separate from native store sealing:
package finalizers seal new native staged outputs before hashing, and Buck2
preserves and validates those modes while already walking metadata for atomic
publication. Existing store outputs are verified and trusted rather than
repaired during use.
After moving native package build tools into execution configuration, a combined
ordinary `zlib` plus C toolchain-smoke build performed one imported GCC
validation in `3.9s`; an immediate repeat completed in `0.0s`, without
building any live bootstrap producer.
Meson, Ninja, and Python are not in the pinned bootstrap closure. They are
defined as native package derivations above the sealed imported facade. The
immediate Meson path uses an explicitly reduced native Python build interpreter
with `zlib` and imported GNU awk/grep/patch; canonical full Python remains
reserved for the normal Nixpkgs-style feature profile. The normalized GCC
wrapper consumes imported role-specific inputs. Its C launchers no longer
inject GCC runtime-library RUNPATHs, allowing C-only `zlib` and `inih`
outputs to retain only Glibc in their runtime closure, while C++ consumers
declare `libstdcxx` and `libgcc` explicitly. The 24-object live export was
uploaded to the configured Foundry CAS, pinned as generated overlay and
Starlark pin records, and imported through public aliases. Toolchain C/C++
smoke and ELF gates, Bubblewrap runtime integration, Perl, and PostgreSQL
`lib`/`bin`/`dev` validation pass through this generation; boundary queries
for toolchains, Python, Meson, `inih`, Bubblewrap, and PostgreSQL contain no
live bootstrap producer, export, or foreign-seed ancestry.

The promoted GNU awk and GNU grep recipes use their published `:bin` facades,
together with the imported final tool profile, instead of stage-zero
self-hosting inputs. Advancing public aliases makes private producer targets
describe a next candidate generation; this is intentional publication flow,
not a dependency from ordinary consumers back into the producer island.

The observed cold costs separate two issues. Under the prior two-walk importer,
first use of ordinary `zlib` plus toolchain smoke targets took `34.3s`, with
repeated verifier walks over large imported objects before Buck2 fingerprinted
the same trees. The one-walk importer removes that duplicate payload pass; its
remaining first-use import scan may still be avoided or shared with published
Buck2 directory metadata or durable receipts. A subsequent fresh `inih` run
took `9:58.7`, primarily rebuilding normal-layer Python and Ninja after
corrected dependency identities changed. That second cost instead motivates
keeping published base identities stable once reviewed. Moving native package
build tools into execution configuration also removes the duplicate configured
native import actions observed when ordinary packages and package-backed
toolchains were built together.

## Recommended Buck2 Integration

The Buck2 fork already has most of the local path semantics:

- store-path declaration in Starlark
- staged output artifacts carrying logical store paths
- materializer-owned publication into `/pkgs/store`
- verification of existing store outputs against recorded artifact values
- source-sealed native outputs whose modes are preserved and validated during
  atomic first publication, without a repair-on-reuse pass

The BuckPkgs prototype implements archive import as a normal verified action
through `pkgs_imported_store_output(...)`, and hydrated bootstrap imports now
use a native Buck2 store-import action that consumes the pinned source manifest
directly. The native action verifies manifest and canonical payload identity
and constructs the Buck artifact value during one store-tree traversal. Direct
CAS tree import or compatible expected directory digests remain the next
optimization for avoiding or sharing that remaining first-use traversal.

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
- ordinary hydrated imports use the native Buck2 registration path rather than
  copying verified trees into staged outputs
- new manifest/publication work should add Buck2-compatible directory identity
  only with an explicit digest-algorithm contract, so a trusted import can
  avoid or reuse the native action's remaining authenticated walk

Buck2's native already-hydrated import action is now the ordinary bootstrap
consumption mechanism. A separate archive/CAS ingestion action can remain a
later integration path for workflows that want Buck2 itself to fetch or decode
substitute bytes; it is not required for the externally hydrated bootstrap
island.

## Optional Future Archive Import Hook

An integrated archive/CAS importer can stay close to existing patterns in the
fork:

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

That extension would keep substitute ingestion aligned with Buck2's existing
artifact model. It is useful for integrated import workflows, but the bootstrap
island's normal path already consumes externally hydrated objects through the
native no-copy registration action.

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
