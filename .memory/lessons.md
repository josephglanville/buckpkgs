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
- Buck2 store paths remain action outputs, so ordinary bootstrap imports need a
  local-only verified projection from an externally hydrated `/pkgs/store`
  object rather than an analysis-time physical-store artifact reference.
- Substitute manifests and imported providers must share one canonical runtime
  closure ordering; otherwise correct pinned metadata can fail verification or
  misalign runtime store artifacts with their logical paths.
