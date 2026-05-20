# Reproducibility

BuckPkgs treats reproducibility as a realization contract, not a best-effort
cleanup pass. A package result should be:

- content-stable for the same declared inputs
- free of transient build-root, host, and wall-clock leakage
- published atomically into `/pkgs/store/...`
- inspectable enough that a mismatch can be diagnosed from artifacts, not
  guessed from logs

This document records the failure modes that have already appeared in the
bootstrap tree and the rules that now exist because of them.

## Core Contracts

### Outputs are either complete or absent

Store publication must never leave a partial final tree behind. Buck2 stages the
materialized store output under a temporary sibling name, restores the staged
mtime metadata, then performs a final rename into place. If materialization or
publication fails, the temporary tree is removed.

Atomic publication prevents crash residue and stale partial outputs. It does not
prove the bytes are deterministic. Content determinism and atomic realization
are separate requirements.

### Realization must normalize ambient process state

Every package-building child process is launched through the normalized command
path in `pkgs-tool`. The current minimum contract is:

- `LC_ALL=C`
- `LANG=C`
- `TZ=UTC`
- `SOURCE_DATE_EPOCH=1`
- `PYTHONHASHSEED=0`
- `PERL_HASH_SEED=0`
- child `umask 022`

These settings are part of build semantics. Locale, timezone, language-runtime
hash iteration, and inherited permission masks can all perturb outputs even when
the declared package graph is unchanged.

### Parallelism is declared input, not host discovery

Package-local `make` parallelism is expressed as `make_jobs`, defaults to a
fixed value of `16`, is passed to build tools as `MAKEFLAGS=-jN`, and participates
in package identity.

Do not derive build parallelism from live host CPU count. Host capacity is an
execution concern; package identity must remain graph-declared. If an upstream
package has a concurrency-sensitive race, compare outputs across explicit
`make_jobs` settings and fix that package or its recipe directly.

### Published metadata is normalized

The final published package tree normalizes mtimes to epoch `1` after install
fixups are complete. This includes:

- regular files
- directories
- symlinks

Symlink mtimes need a distinct non-following update path. Normalizing only files
and directories leaves wall-clock link creation times behind.

## Path Leakage Rules

### Transient build roots must not reach final payloads

Absolute Buck scratch paths, recipe work directories, and repo checkout paths
may exist while the build runs. They are bugs once serialized into installed
artifacts.

Final store scans should catch at least:

- `buck-out/v2/tmp`
- `pkgs-configure-make-install`
- the live repo checkout path
- package-specific scratch roots discovered during debugging

A path is not harmless just because it first appears in a temporary command
line. Generated headers, archives, installed makefiles, configure state, and
driver binaries can all copy that path into the payload.

### Prefix remapping belongs in execution, not persisted recipe metadata

Injecting deterministic path flags through broad ambient `CFLAGS` can change
configure behavior and can itself become serialized recipe state.

The safer pattern is:

1. Preserve upstream package flag structure.
2. Inject remapping at execution boundaries through transient wrappers or
   package-specific preserved environment.
3. Verify the final payload, not just the compile command line.

GCC exposed both sides of this:

- `--with-debug-prefix-map=...` was itself serialized into GCC artifacts.
- `-fdebug-prefix-map` fixed debug provenance but not every serialized file
  name.
- target libraries also required `-ffile-prefix-map` to eliminate absolute
  build include roots from archive contents.

### Sanitizers must remove only transient data

Libtool `.la` files are part of the installed packaging surface. Their
`dependency_libs` fields can freeze transient `-L...` scratch paths even when the
compiled binaries are clean.

Sanitization must remove transient workdir-backed search paths while preserving
valid declared store references. Do not blank metadata wholesale just to make a
scan pass.

## Practical Lessons from the Bootstrap Tree

- Bash needed a narrow installed-Makefile fix. A broader rewrite of its build
  directory semantics broke recursive build behavior.
- A corrected consumer can still publish poisoned bytes if an older dependency
  tree remains dirty. Rebuild chains matter.
- PATH-level compiler wrappers do not automatically cover compiler bootstrap
  phases that invoke freshly built tools by absolute path.
- Broad store-wide scans mix new signal with older poisoned artifacts. Prefer
  inspecting fresh store roots from the current rebuild when diagnosing a new
  mismatch.
- Deterministic archive and compression metadata still need direct artifact
  checks. Static archives should have deterministic owner/time metadata, and gzip
  headers should not retain build-time mtimes.

## Package Authoring Rules

When adding or changing a package recipe:

1. Keep all realization-affecting concurrency, flags, and tool choices declared.
2. Avoid host discovery in recipe logic unless it is explicitly part of the
   package ABI.
3. Prefer transient wrappers or narrowly preserved environment to mutating global
   package flags.
4. Treat installed text metadata, configure products, archives, and wrapper
   scripts as possible leak surfaces.
5. Normalize only with evidence. Do not paper over mismatches by deleting or
   rewriting installed content broadly.
6. Rebuild dependent consumers when an upstream reproducibility bug could have
   been copied into their outputs.

## Verification Expectations

For realization-layer changes:

- run the targeted `pkgs-tool` tests
- run formatting and whitespace checks
- build representative package targets that exercise the changed contract
- inspect fresh store outputs directly

For bootstrap-sensitive changes:

- rebuild the relevant ladder stage
- inspect the newly published store tree for path, metadata, and host leaks
- rebuild the final closure checks when the affected inputs flow into them

At minimum, final artifact review should answer:

- Did publication remain atomic?
- Are final mtimes normalized?
- Are transient workdir paths absent?
- Are host-specific strings absent unless intentionally part of the ABI?
- Are archive and compression metadata deterministic where applicable?
- Did any stale older artifact get mistaken for a fresh regression?

Passing tests are evidence, not a substitute for final artifact inspection.
