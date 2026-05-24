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
mtime and permission metadata, verifies that a new native producer already
sealed its regular files and directories, then performs a final rename into
place. If materialization or publication fails, the temporary tree is removed.
An existing store path is verified and reused; normal use does not repair it.

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
fixed value of `64`, is passed to build tools as `MAKEFLAGS=-jN`, and participates
in package identity.

When a future change already forces a bootstrap-tree rebuild, batch any intended
declared-parallelism retuning into that same rebuild rather than paying a
standalone invalidation cost for it.

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

Published store trees are read-only. Package finalizers seal their staged
outputs before Buck hashes them. Buck2 does not apply ordinary writable-output
normalization while hashing or verifying store outputs; during atomic
publication it preserves and validates the producer's modes while already
copying required metadata. Changing a defective published tree requires a new
package identity; the ordinary materialization path does not rewrite it.

### Named outputs default to code-bearing interfaces

For newly ported packages, `bin` contains runnable programs, `lib` contains
runtime shared libraries plus indispensable loaded runtime data, and `dev`
contains headers, static archives, and build metadata. Reserve `out` for a
compound runtime payload that cannot yet be separated without losing a
required runtime edge. A split `bin` output that loads sibling shared
libraries must declare its dependency on the package's primary `lib` output.

Do not create standalone `man`, `doc`, or `info` projections unless a package
explicitly needs those outputs. Recipes with mixed upstream installations
should use explicit primary output keep-lists so documentation does not fall
into a catch-all runtime output; retain only runtime data directories that are
actually required. Existing bootstrap-facing output labels remain stable
during PostgreSQL bring-up and are tracked for a later alignment pass.

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

## External Practice Worth Adopting

Debian's reproducible-builds work treats byte reproducibility as an adversarial
artifact property, not as a "clean enough" recipe style. The useful habits for
BuckPkgs are:

- vary wall clock, build path, directory order, locale, timezone, username,
  hostname, CPU topology, and related ambient state when chasing a mismatch
- first make the producing tool deterministic, then use format-specific
  normalization only when the format or upstream producer genuinely requires it
- compare final artifacts recursively and keep the diagnosis attached to bytes,
  not just to a suspicious log line
- record enough environment and dependency identity that a later rebuild has a
  concrete target to reproduce

BuckPkgs already covers part of that internally through the replayed
`[reproducible]` subtarget and normalized package execution environment. The
remaining authoring discipline is to keep package recipes within the same model
instead of reintroducing ambient host state through a build-system escape hatch.

### Research distilled into BuckPkgs policy

The strongest portable guidance from Debian, reproducible-builds.org, and the
major build systems reduces to a few rules:

- Define reproducibility against a specific build environment, not against
  whatever happens to be installed on the current machine.
- Keep audit data separate from the produced payload. Debian's `.buildinfo`
  work is useful evidence for that separation, but BuckPkgs should not mirror it
  wholesale: the Buck graph already declares rebuild inputs more precisely than
  an after-the-fact package metadata file can.
- Treat `SOURCE_DATE_EPOCH` as the timestamp contract for embedded build dates
  and timestamp clamping. Wall-clock time should not enter package bytes unless
  the package ABI explicitly says it does.
- Treat build paths, archive metadata, filesystem order, uid/gid/mode capture,
  locale, timezone, username, hostname, umask, and runtime hash iteration as
  normal reproducibility threat classes, not rare one-off bugs.
- Prefer producer fixes over post-processing. Use format-specific normalization
  where the format itself carries irrelevant metadata or upstream tooling gives
  no better hook.
- Use recursive artifact comparison to explain mismatches. Logs can point at a
  suspect step; final bytes decide whether a build is reproducible.

Path remapping deserves one extra rule. Compiler prefix-map flags are practical
today and should stay part of the BuckPkgs toolbox. The broader
`BUILD_PATH_PREFIX_MAP` idea is useful as a design reference, but it is still not
the foundation we should assume every tool honors. BuckPkgs should keep explicit
wrapper and verifier coverage instead of betting the package graph on that
cross-tool contract.

## Build-System Playbooks

### Autotools

- For out-of-source builds, invoke `configure` through a relative path from the
  build directory. Absolute source checkout paths are easy to serialize into
  generated files.
- Treat `config.status`, generated headers, installed `*-config` helpers,
  libtool archives, and generated Makefiles as payload surfaces, not throwaway
  build metadata.
- Prefer upstream support for `SOURCE_DATE_EPOCH`. If a configure script or
  helper emits wall-clock text, patch that producer narrowly instead of
  post-rewriting broad output trees.
- Prefer `AX_BUILD_DATE_EPOCH` or equivalent project-native support when
  Autoconf logic needs a build date. Do not let `configure` synthesize fresh wall
  clock strings and hope an install-time scrub catches every consumer.
- Keep deterministic archive behavior explicit when an Autotools package
  overrides the usual tool variables. `ar` and `ranlib` should remain in
  deterministic mode; package recipes should not quietly reintroduce host-owned
  archive metadata.
- Keep build parallelism declared through `make_jobs`; do not let package-local
  scripts derive it from live host CPU count.

### CMake

- Timestamp-producing project logic should flow through `SOURCE_DATE_EPOCH` or
  avoid embedding build time entirely. Review `string(TIMESTAMP ...)`,
  generated headers, and version banners when a package differs only by text
  payloads.
- When CMake itself creates archives, use archive APIs that accept explicit
  mtimes rather than letting source-tree mtimes or wall clock values leak into
  the result.
- Treat `CMAKE_SOURCE_DIR`, `CMAKE_BINARY_DIR`, generated export files, and
  configured scripts as potential build-path leak points.
- Projects that download and extract nested archives during configuration need
  explicit timestamp semantics; archive extraction behavior is part of the
  reproducibility contract, not an incidental fetch detail.
- Package-generated archives and installers still need separate artifact checks.
  A reproducible CMake configure step does not prove a downstream tarball, zip,
  or CPack-style payload is byte-canonical.
- CPack-like packaging needs its own policy surface: pinned uid/gid handling,
  chosen archive format, explicit compression settings, and no hidden host-driven
  threading or defaults that can vary between environments.

### Meson

- Meson should be treated as reproducible by default only when the surrounding
  toolchain and project-authored generators are also deterministic.
- For packager-controlled builds, prefer Meson's plain build mode so BuckPkgs
  remains the authority for compiler and linker flags instead of merging them
  with Meson's convenience defaults.
- `pkgs_meson_install_package(...)` enforces out-of-source setup with
  `--buildtype=plain`, `--backend=ninja`, `--libdir=lib`,
  `--auto-features=disabled`, `--wrap-mode=nodownload`, and
  `--install-umask=022`; packages must declare any intentional override in
  `meson_args`, which participates in store identity.
- Meson builds receive an explicit `meson_jobs` input and installation runs
  with `--no-rebuild`, so the install phase cannot silently schedule another
  backend build outside the declared parallelism policy.
- Audit `run_command()`, custom targets, generated configuration files, and any
  helper that reads the wall clock, the live source/build directory, `uname`, or
  other host identity.
- Prefer Meson's normal out-of-source model; do not tunnel around it with ad hoc
  shell glue that writes absolute scratch paths into installed outputs.
- If a Meson project is not reproducible under BuckPkgs' normalized environment,
  treat that as a package or upstream bug to prove and fix, not as a reason to
  weaken verification.
- Meson's release-archive discipline is worth copying for substitution inputs:
  package the declared source state, then validate that the produced source
  archive can complete the expected build/test/install loop before trusting it
  as a reusable bootstrap artifact.

## BuckPkgs Action List

These are the concrete repository changes that follow from the research above.
They are intentionally phrased as implementation work, not as general
principles:

1. Define substitute/import manifests for byte-perfect realized store objects.
   They should capture artifact digest, store identity, target system, declared
   source digests, closure references needed for validation, archive digest, and
   verifier/schema version without duplicating the Buck graph as a parallel
   rebuild description.
2. Grow the current archive verifier beyond GNU `ar` and gzip. The next useful
   classes are tar-like payloads, zip-like payloads, and compressed source or
   bootstrap substitute archives whose uid/gid, member order, mtime, or header
   data can still vary.
3. Add package-family authoring helpers or lint rules for Autotools, CMake, and
   Meson. The checks should catch wall-clock producers, absolute build-path
   expansion, host-discovered parallelism, non-deterministic archive creation,
   and package-local overrides that bypass BuckPkgs' normalized tools.
4. Keep path remapping centralized in wrappers and package execution helpers.
   Recipes should opt into narrow exceptions only when a package's own build
   system genuinely needs them.
5. Add a reprotest-style perturbation harness for selected representative
   packages. Rebuild under varied checkout roots, temp roots, username/hostname
   surfaces where practical, locale/timezone, umask, and explicit `make_jobs`
   settings, then compare final trees recursively.
6. Make source-substitution artifacts first-class and byte-perfect. Bootstrap
   substitutes should be verified archives with recorded provenance and checked
   re-realization semantics, not merely convenient tarballs.
7. Extend static scans for common leak patterns in both recipes and fresh output
   trees: `__DATE__`, `__TIME__`, wall-clock shell usage, repo/workdir paths,
   generated host banners, and known archive/compression metadata hazards.
8. Keep whole-closure rebuilds in the workflow whenever a fixed producer may
   have already contaminated downstream artifacts. A green leaf package is not
   proof that the closure recovered from older non-deterministic bytes.

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

Every package target also exposes a `[reproducible]` subtarget. Building it
replays only that package into a disposable tree with the same declared inputs,
then verifies that the replayed tree matches the published store tree in shape,
file bytes, symlink targets, permission modes, and normalized mtimes. This keeps
layer-by-layer byte reproducibility checks local to the package under review
instead of forcing a whole-bootstrap rebuild for every proof point.

The replay artifact remains an ordinary Buck output, so Buck may normalize its
write bits even though the published store tree retains source-sealed modes.
The permission comparison therefore excludes write bits only; it still checks
executable and special mode bits so recipe-visible mode drift is not masked.

Package targets also expose an `[archive_metadata]` subtarget. It scans the
published tree for static archives and gzip streams, then rejects the common
non-canonical metadata classes that otherwise survive into final bytes:

- GNU `ar` members with non-zero timestamp, uid, or gid fields
- gzip headers that retain a stored timestamp or original filename

Use it for packages that install `.a`, `.gz`, generated docs, manpages, source
bundles, or other archive-like payloads. It is intentionally narrower than a
general diffing tool; it exists to make a high-value authoring check cheap enough
to run routinely.

Examples:

```text
buck2 build root//development/libraries/glibc:out_stage1[reproducible]
buck2 build root//development/compilers/gcc:out_stage1[reproducible]
buck2 build root//development/libraries/zlib:out[archive_metadata]
```

For bootstrap-sensitive changes:

- rebuild the relevant ladder stage
- inspect the newly published store tree for path, metadata, and host leaks
- rebuild the final closure checks when the affected inputs flow into them

For package-specific shared-library linkage, declare `link_inputs` rather than
embedding ad hoc store `-L` or RUNPATH flags in package environment values.
This keeps the dependency in package identity and runtime closure while the
realization layer supplies deterministic store-backed link/runtime lookup.
The Meson realization layer carries those paths through setup linker flags so
Meson's install step preserves the declared installed RUNPATH.

For package metadata lookup, declare `PkgConfigInfo` search roots on produced
outputs and consume them through dependency roles. Standard builders set both
`PKG_CONFIG_PATH` and `PKG_CONFIG_LIBDIR`, including empty values when no roots
are declared, so host default metadata is not an implicit input. Named output
splits are identity-bearing outputs of the same realization action; relocated
`*.pc` directory variables are repaired before each output is sealed.

At minimum, final artifact review should answer:

- Did publication remain atomic?
- Are final mtimes normalized?
- Are transient workdir paths absent?
- Are host-specific strings absent unless intentionally part of the ABI?
- Are archive and compression metadata deterministic where applicable?
- Did any stale older artifact get mistaken for a fresh regression?

Passing tests are evidence, not a substitute for final artifact inspection.

## Verified Package Checklist

Mark a package complete only after its current recipe passes both
`[reproducible]` and `[archive_metadata]`, plus any applicable bootstrap
boundary check.

- [x] `//shells/bash:bin_stage0` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and `bin_stage0_seed_free` after
  replacing the host pipe-size probe and omitting installed build metadata.
- [x] `//tools/text/gnused:bin_stage0` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and `bin_stage0_seed_free`.
- [x] `//tools/text/gnugrep:bin_stage0` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and `bin_stage0_seed_free`.
- [x] `//tools/text/gawk:bin_stage0` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and `bin_stage0_seed_free`.
- [x] `//tools/text/gnugrep:bin` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:gnugrep_bin_seed_free`; verified stage0 `grep` supplies
  its configure-time self-hosting dependency.
- [x] `//tools/text/gawk:bin` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:gawk_bin_seed_free`; verified stage0 `awk` supplies
  its configure-time self-hosting dependency.
- [x] `//tools/text/gnupatch:bin` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:gnupatch_bin_seed_free`.
- [x] `//development/libraries/zlib:out` and `:dev` — verified 2026-05-23
  with `[reproducible]` and `[archive_metadata]` for both projected outputs;
  fresh store publications at
  `/pkgs/store/1ba90ca09655453e4ef3cc5e30a7b5ad-zlib-1.3.2` and
  `/pkgs/store/3a75b89b0d616bf7b9c2782d42e28cea-zlib-1.3.2-dev` contain no
  writable regular file or directory. Native `pkg-config --cflags --libs
  zlib` resolves headers from `:dev` and libraries from `:out`.
- [x] `//development/tools/pkg-config:bin` and `:dev` — verified 2026-05-23
  with `[reproducible]` and `[archive_metadata]` for each exported output;
  native `pkgconf` supplies the successful isolated zlib metadata query.
- [x] `//development/interpreters/python:build_interpreter` — verified
  2026-05-23 with `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:python_build_interpreter_seed_free`; the recipe discovers
  zlib through native `pkgconf` and declared `zlib:dev` metadata while
  retaining `zlib:out` as its link/runtime input. Direct execution of
  `import zlib` without `LD_LIBRARY_PATH` succeeds, and fresh publication at
  `/pkgs/store/65dcb17645c644175972966c5150d8ff-python3-build-3.13.10`
  contains no writable regular file or directory.
- [ ] `//development/interpreters/python:bin` — reserved for a normal full
  Python contract; Nixpkgs' default CPython enables `bzip2`, `libffi`,
  `libuuid`, `ncurses`, `xz`, `zlib`, `openssl`, `sqlite`, `mpdecimal`,
  `expat`, `gdbm`, and `readline`, unlike `python3Minimal`.
- [x] `//development/tools/build-managers/ninja:bin` — verified 2026-05-23
  with `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:ninja_bin_seed_free`; fresh publication at
  `/pkgs/store/f433e7df6f1fdfc1add24d4757760b96-ninja-1.13.2` is sealed.
- [x] `//development/tools/build-managers/meson:bin` — verified 2026-05-23
  with `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:meson_bin_seed_free`; its installed wrapper embeds declared
  native Bash/Python paths via identity-bearing installation arguments, and
  `/pkgs/store/ecdd319136967b9ee7aa5ed7e109074e-meson-1.9.1` is sealed.
- [x] `//development/libraries/inih:out` — verified 2026-05-23 with
  `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:inih_out_seed_free` plus
  `bootstrap/tests:inih_native_graph_foreign_build_free`; built through
  corrected native Meson, and revalidated 2026-05-24 after Meson
  `link_inputs` RUNPATH handling was generalized.
- [x] `//tools/compression/zstd:lib` and `:dev`,
  `//tools/compression/lz4:lib` and `:dev`,
  `//development/libraries/ncurses:lib` and `:dev`, and
  `//development/libraries/readline:lib` and `:dev` — verified 2026-05-24
  with `[reproducible]` and `[archive_metadata]` for each output, plus
  seed-free checks for the runtime outputs used by PostgreSQL.
- [x] `//development/tools/misc/gnum4:bin`,
  `//development/interpreters/perl:lib` and `:bin`,
  `//development/tools/parsing/bison:lib` and `:bin`, and
  `//development/tools/parsing/flex:bin` — verified 2026-05-24 with
  `[reproducible]` and `[archive_metadata]`, plus seed-free checks for the
  executable outputs used in PostgreSQL's build graph.
- [x] `//development/libraries/libcap:lib` and `:dev` — verified 2026-05-24
  with `[reproducible]`, `[archive_metadata]`, and seed-free checks for both
  projections; `:lib` retains libraries only and `:dev` retains headers,
  static archives, and pkg-config metadata only.
- [x] `//tools/sandboxing/bubblewrap:bin` — verified 2026-05-24 with
  `[reproducible]`, `[archive_metadata]`, and
  `bootstrap/tests:bubblewrap_bin_seed_free`; `bwrap --version` runs using
  its declared `libcap:lib` RUNPATH and the projected output contains only
  the executable.
- [x] `//servers/sql/postgresql:lib`, `:bin`, and `:dev` — verified 2026-05-24
  with `[reproducible]`, `[archive_metadata]`, and seed-free checks for all
  three projections; runtime/documentation selective policy holds and
  installed PGXS metadata normalizes transient Configure/Make work paths.
