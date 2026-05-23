# Reproducibility Lessons

- Injecting deterministic flags through ambient `CFLAGS` can change upstream
  configure behavior and persist unwanted metadata. Realization-time compiler
  wrappers kept the flag effect while preserving package defaults.
- A path can be harmless inside a transient build tree and still be a release
  risk if generated files copy it into the installed payload. Final store scans
  are required before treating a source as benign.
- Bash needed a narrow installed-Makefile fix, not a broad rewrite of its global
  build-directory notion. The broader change broke recursive build invariants.
- Atomic store publication prevents partial outputs from surviving failures,
  but it does not solve semantic nondeterminism. Content stability and atomic
  realization are separate contracts.
- Store immutability must originate in package finalization, not in a
  repair-on-access operation. Buck2 was making source-sealed native outputs
  writable in `build_entry_from_disk` while hashing and verifying store paths;
  store outputs must skip that normalization, then atomic publication preserves
  and validates the already-sealed modes. An already published path is trusted
  or invalidated by a new identity.
- Store publication need not chmod every copied regular file: the existing
  `std::fs::copy` path preserves producer file permissions. Directory modes
  still require final restoration because temporary directories stay writable
  while their children are populated; error cleanup must reopen only temporary
  sealed directories before removing them.
- Libtool archives are a packaging boundary, not harmless metadata. Their
  `dependency_libs` fields can freeze transient build roots into installed
  payloads even when the compiled binaries themselves are already clean.
- Filesystem metadata can lag behind content determinism. Keeping source mtimes
  during the build is useful, but published package trees should not retain
  wall-clock `make install` times.
- Symlinks need their own metadata path. Skipping them leaves wall-clock link
  mtimes behind even when every regular file and directory has been normalized.
- A clean consumer can still publish dirty bytes when one linked dependency is
  poisoned. Glibc stopped exporting its own build root, but older GCC runtime
  objects still reintroduced Buck scratch paths into glibc shared objects.
- PATH-level compiler wrappers do not automatically cover compiler bootstrap
  steps that invoke freshly built compilers by absolute path, such as GCC's
  internal `xgcc` phases.
- Environment normalization is incomplete without permission normalization.
  Child build commands need a fixed `umask`, otherwise identical bytes can
  still publish with different file modes.
- Language-runtime hash seeds are build inputs when Python or Perl-based
  generators sit anywhere in the tool surface. They need explicit values even
  if the immediate package rules do not mention them directly.
- Broad store-wide path scans mix current signal with old poisoned outputs.
  Random tempdir suffixes are a useful discriminator for pre-fix artifacts.
- GCC's `--with-debug-prefix-map=...` argument is itself serializable build
  state. Feeding remap flags through preserved environment instead keeps the
  effect without baking the scratch root into `configargs.h` or driver payloads.
- `-fdebug-prefix-map` only fixes debug-location provenance. Header paths
  embedded through file-name serialization in target libraries require
  `-ffile-prefix-map` as well, which is why the first GCC stage1 archive pass
  still leaked absolute `libstdc++` include roots.
- Store-substitute extraction must validate archive structure before writing:
  hash validation alone does not prevent a signed archive containing a symlink
  followed by a descendant file entry from escaping its destination tree.
- A Buck import test needs a store path with no live package producer; exporting
  and reimporting one live package only exercises existing-store verification,
  not first publication of the imported store object.
- Buck2 store paths remain action outputs, but ordinary bootstrap imports do not
  need a staged copy: a native Buck2 import action can register an externally
  hydrated `/pkgs/store` object by accepting the pinned source manifest as its
  direct input. With legacy canonical-payload hashes, the importer can validate
  the manifest contract and canonical bytes while constructing Buck2's
  artifact value from the same physical traversal; a Buck2-compatible
  directory digest or trusted receipt is instead needed to avoid or share that
  remaining walk.
- The remaining import cost and ordinary invalidation cost are different
  problems. First-use imported toolchains still require one authenticated
  store-tree traversal per uncached imported artifact. Content-based outputs
  and source manifests are still required, but eligibility alone did not merge
  a local custom native action across configurations. Declare native package
  build tools as execution dependencies so the normal package/toolchain path
  shares the exact imported action node. Changing published base identities can
  then force costly but legitimate rebuilds of normal tools such as Python and
  Ninja. Optimize the remaining scan with reusable Buck2 directory metadata or
  receipts, and avoid triggering the latter casually.
- Substitute manifests and imported providers must share one canonical runtime
  closure ordering; otherwise correct pinned metadata can fail verification or
  misalign runtime store artifacts with their logical paths.
- Importing only final compiler wrappers is not enough for ordinary package
  builds. A configure/make package needs an imported build-tool profile
  containing its final shell, make, and basic Unix utilities as well.
- A reusable C compiler wrapper must not blindly add C++ compiler-runtime
  `RUNPATH` entries to every C library. Republish wrapper-policy changes from
  imported base inputs, then C-only packages can drop the compiler runtime
  edge while C++ launchers continue to retain it.
- Meson installation must not trigger an implicit rebuild. Running explicit
  `meson compile --jobs N` followed by `meson install --no-rebuild` keeps
  parallelism in the declared package identity instead of backend discovery.
- Bootstrap transport belongs behind canonical package labels. Ordinary
  recipes should not encode whether a tool is live, foreign-seeded, or supplied
  by a hydrated substitute; boundary checks and publication rules may.
- A provider-level seed cut does not sever Buck action ancestry: a promoted
  self-hosting output can be byte-clean while ordinary consumers still analyze
  its stage0 construction graph. The GCC precedent applies here: publish the
  validated output and expose only its imported façade to ordinary builds.
- Because bootstrap producers live under ordinary-looking `development/`,
  `tools/`, and `shells/` paths, naming convention is not a graph boundary.
  Restrict live outputs with an exact visibility allowlist and separately
  restrict substitute transport targets to canonical façade aliases.
- Seal bridge-shaped targets too: a seed-check stamp or per-object export is a
  dependency edge into its producer unless its visibility is restricted.
- A missing higher-layer build tool is not justification for widening the
  foreign seed. Build Python, Ninja, Meson, and their prerequisites as native
  packages from the sealed base unless an explicit bootstrap expansion is
  approved.
- Once a published baseline exists, promoted GNU grep and GNU awk should
  self-host through the published façade rather than stage0 inputs. Continue
  to require each republished output to pass the foreign-seed boundary.
- Self-hosted publication is generational: an isolated candidate may build
  from the currently published façade, then become the next pinned façade.
  After the alias advances, analyzing the private recipe can legitimately
  produce a different candidate identity; ordinary consumers must remain
  attached only to the pinned import generation.
- CPython installs generated `_sysconfigdata_*.py` and its Makefile, so in-tree
  scratch paths in `abs_srcdir` and `abs_builddir` are package outputs, not
  harmless build logs; make those values relative before bytecode is emitted.
- GNU deterministic archives may contain a `//` long-filename table whose
  numeric metadata fields are canonically blank. Archive validation must
  recognize that format without accepting timestamps on the metadata member.
- Nixpkgs' `python3Minimal` deliberately omits optional libraries including
  `zlib`, while Meson imports Python's `gzip` path in ordinary execution.
  A zlib-enabled reduced interpreter can unblock native build tools, but it
  must not be exposed as the canonical full `python3` package.
- Declaring a shared library in a runtime closure does not make the dynamic
  loader find it. Package-scoped `link_inputs` must inject store-backed
  link-search/RUNPATH flags and carry that library's runtime closure;
  CPython's zlib-linked extension modules otherwise fail during installation.
- Build-time Make variables are not automatically install-time variables.
  Generated installed wrappers that embed declared interpreters must receive
  those store-backed values explicitly during installation.
- Descriptor-backed install arguments affect installed bytes just like plain
  install arguments. Omitting them from store identity lets a corrected
  realization collide with a previously published divergent store object.
