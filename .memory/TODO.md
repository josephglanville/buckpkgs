# Reproducibility Board

## Package Substitution

- [x] Define and parse a versioned store-object manifest that commits to store
      identity, archive bytes, realized-tree identity, and closure references.
- [x] Implement canonical substitute archive export and hydration with atomic
      publication under `/pkgs/store`.
- [x] Implement imported `PkgsPackageInfo` providers backed by
      manifest-verified store-object archives.
- [x] Publish a finalized bootstrap closure manifest and point
      `toolchains//:cxx_pkgs` at imports rather than live turnover targets.
- [x] Prove a clean ordinary consumer can hydrate/import the bootstrap closure
      without analyzing or executing live bootstrap stages.
- [x] Build a real PostgreSQL-path package dependency (`zlib`) using an
      imported final bootstrap tool profile rather than foreign-seed or live
      turnover targets.
- [x] Re-prove deterministic Meson realization with real upstream `inih` `r62`
      through the expanded pinned closure. The local assembled closure hydrates
      successfully, source analysis contains no live
      `gawk`/`gnugrep`/`gnupatch` producer ancestry under Python, and a fresh
      ordinary `inih` build completed through Python, Ninja, and Meson without
      entering the bootstrap producer graph.
- [x] Prove descriptor-backed installation values change package identity and
      preserve reproducibility through the corrected Meson wrapper and `inih`.
- [ ] Define canonical `root//development/interpreters/python:bin` only after
      implementing a full native dependency profile comparable to Nixpkgs
      `python3`: `bzip2`, `libffi`, `libuuid`, `ncurses`, `xz`, `zlib`,
      `openssl`, `sqlite`, `mpdecimal`, `expat`, `gdbm`, and `readline`.

## Active

- [x] Rebuild GCC stage1 with combined
      `-ffile-prefix-map=@PKGS_WORK_DIR@=.` and
      `-fdebug-prefix-map=@PKGS_WORK_DIR@=.` target-library remaps, then verify
      `libstdc++.a` and `libstdc++fs.a` no longer serialize Buck scratch include
      paths.

## Checked

- [x] `root//bootstrap/exports:linux_x86_64_bundle` exported finalized wrapper
      roots for GCC and Binutils, and the checked-in
      `bootstrap/substitutes/linux_x86_64/` closure/object manifests compare
      byte-for-byte with that generated bundle metadata.
- [x] `pkgs_hydrate_store_closure` hydrated the pinned fourteen-object
      `bootstrap-linux-x86_64` closure into a disposable store root, proving the
      bundle is complete and internally consistent before ordinary import use.
- [x] `toolchains//tests:gcc_smoke` and `toolchains//tests:hello_world_c` build
      through `toolchains//:cxx_pkgs` using canonical
      `root//development/{compilers/gcc:bin,tools/misc/binutils:bin}` aliases
      backed by pinned substitutes, and
      `cquery deps(toolchains//:cxx_pkgs)` contains no live GCC, Binutils,
      Bash, Glibc, or bootstrap export targets.
- [x] Nixpkgs PostgreSQL lists `zlib` in direct `buildInputs`; the new
      `root//development/libraries/zlib:out` build consumes canonical imported
      `bash`, GNU Make, Binutils/GCC wrappers, Coreutils, Findutils, GNU sed,
      and Glibc, produces shared/static `libz` plus `zlib.pc`, passes
      `root//bootstrap/tests:zlib_out_seed_free`, and its transport-only
      substitute labels remain behind canonical package façades.
- [x] The restarted full bootstrap rebuild after the host power loss completed
      successfully for
      `//bootstrap/tests:final_base_seed_free` and
      `//bootstrap/tests:final_base_pkgs_interpreters`; every newly published
      final store root inspected during that run retained epoch `1` mtimes and
      stayed free of the targeted Buck scratch/workdir leak signatures.
- [x] The `umask` regression now forces a hostile parent `0077` mask before
      launching the normalized child process, and the full `pkgs-tool` suite
      passes both normally and when the parent shell itself runs under
      `umask 077`.
- [x] Child build environments are normalized with fixed locale, timezone, and
      `SOURCE_DATE_EPOCH`.
- [x] Build-path remapping is injected through transient compiler wrappers
      rather than installed package metadata.
- [x] Bash stage0 no longer installs an absolute scratch `BUILD_DIR`.
- [x] Existing gzip payloads do not carry nonzero gzip header mtimes.
- [x] Sampled static archives use deterministic archive metadata.
- [x] Repo search found no explicit `--build-id=uuid`, `__DATE__`, or
      `__TIME__` package rules.
- [x] Sampled fresh ELF outputs use hash-style build IDs rather than
      UUID-style IDs.
- [x] Child build commands now exec under `umask 022`, and the unit suite
      verifies the child-created file mode.
- [x] Child build environments now pin `PYTHONHASHSEED=0` and
      `PERL_HASH_SEED=0`, with integration coverage proving ambient parent
      values are overwritten.
- [x] Store scans found no direct leak of the live host kernel release string
      or current wall-clock build date spellings.
- [x] Fresh Binutils output exposed a real `.la` nondeterminism bug; the
      realization layer now strips only transient workdir `-L...` entries while
      preserving valid store-backed dependency references.
- [x] Realized package trees and wrapper trees now normalize output mtimes after
      post-install fixups so ambient build time does not survive publication.
- [x] Symlink mtimes are normalized with `utimensat(..., AT_SYMLINK_NOFOLLOW)`
      so filesystem-tree comparisons do not retain wall-clock link creation
      times.
- [x] Fresh staged `coreutils:bin_stage0` output confirms `pkgs-tool` now
      normalizes action-output directories and regular files to the fixed epoch
      before store publication.
- [x] Buck2 store publication was dropping staged mtimes while building the
      atomic temp tree; the local Buck2 fork now restores staged modified times
      for files, directories, and symlinks before the final rename, with focused
      materializer tests.
- [x] Native package finalizers now seal staged package trees. Buck2 no longer
      normalizes native store outputs writable while hashing or verifying them;
      atomic publication preserves and validates producer-sealed modes during
      its existing metadata pass, without repair on reuse.
- [x] Republished and reverified the native path through `zlib`, Python, Ninja,
      Meson, and `inih` under the immutable-publication identity bump; each
      fresh store root contains no writable regular file or directory.
- [x] The live bootstrap rebuild published new `gzip`, `gnugrep`, `bash`,
      `diffutils`, and `coreutils` store roots whose ctimes are current while
      their final mtimes remain normalized to epoch `1`, confirming the Buck2
      materializer preserves normalized store mtimes in real package flows.
- [x] Fresh staged Binutils output now sanitizes transient Buck scratch `-L...`
      references out of installed `.la` files; the remaining rebuild failure
      was an older poisoned published `/pkgs/store/...-binutils-2.46.0` tree
      colliding with the corrected artifact.
- [x] Freshly published
      `/pkgs/store/b9b2260ddbb2150d6b090c822a1f3129-binutils-2.46.0`
      confirms the corrected `.la` files remain clean after final publication:
      no transient Buck scratch or `/tmp` search paths survive, while valid
      store-backed `dependency_libs` references remain.
- [x] Fresh Glibc-generated `.gz` payloads are byte-identical across sampled
      store trees and their gzip header MTIME field is `0`; the differing dates
      shown by `gzip -lv` came from staged filesystem mtimes, not gzip bytes.
- [x] Current package store outputs do not emit `.tar`, `.tgz`, `.zip`, `.jar`,
      or `.pyc` payloads, so those archive/bytecode normalization paths are not
      an active surface in this bootstrap tree today.
- [x] Fresh staged Binutils and Glibc static archives show deterministic `ar`
      member metadata (`0/0` ownership and epoch timestamps).
- [x] Freshly published GCC stage0 proved the remaining scratch leak came from
      GCC serializing the literal `--with-debug-prefix-map=/...buck-out...=.`
      configure argument into `configargs.h` and driver binaries; the GCC rule
      now uses an env-fed `DEBUG_PREFIX_CFLAGS_FOR_TARGET` plus a small source
      patch so the remap flag is not part of serialized configure arguments.
- [x] Freshly published GCC stage1 exposed a separate target-library issue:
      `libstdc++.a` and `libstdc++fs.a` still captured absolute build include
      paths through file-name serialization, so the env-fed target remap now
      carries both `-ffile-prefix-map` and `-fdebug-prefix-map`.
- [x] Focused rebuild of
      `/pkgs/store/9eafad3da1a1d6b16a11809bcc8aec55-gcc-15.2.0`
      published with epoch `1` mtime and no remaining `buck-out/v2/tmp`,
      `pkgs-configure-make-install`, `with-debug-prefix-map`, or
      `DEBUG_PREFIX_CFLAGS_FOR_TARGET` strings in the final payload scan.
- [x] The follow-up GCC stage0 rebuild with combined file/debug target remaps
      published
      `/pkgs/store/864eaa6ce95bd2a9e0653c5210c93c97-gcc-15.2.0`
      with epoch `1` mtimes and no final-store hits for Buck scratch roots,
      serialized debug-prefix configure state, or the live host LTO plugin
      path observed only in the transient build command line.
- [x] Tree traversal now sorts directory entries by name across source copying,
      source-overlay composition, archive sanitation, verification passes,
      header cleanup, read-only sealing, and mtime normalization so filesystem
      enumeration order does not perturb package contents or diagnostics.
- [x] Package-local `make` parallelism is now an explicit declared input rather
      than `std::thread::available_parallelism()`: package rules default to a
      fixed `make_jobs = 64`, pass `--make-jobs` into `pkgs-tool`, and fold that
      value into store identity.
- [x] `pkgs-tool` rejects zero make jobs and integration coverage confirms an
      explicit `--make-jobs 7` reaches child `MAKEFLAGS` as `-j7`.
- [x] Symlink assignments now use the same sorted order in package action
      arguments that they already used in recipe hashing, avoiding a latent
      digest-versus-execution ordering mismatch.
- [x] All package-building child processes in `pkgs-tool` now flow through the
      normalized command launcher; direct process construction remains confined
      to that single helper.
- [x] A rule/tooling audit found the remaining explicit `/usr/bin/...` host-tool
      exposure only in the intentional `foreign_seed` wrappers, which the final
      seed-free closure checks are already designed to reject from published
      package outputs.
- [x] Repo-owned rules, patches, and helpers do not hardcode their own
      `make -j`, `nproc`, or `getconf _NPROCESSORS_ONLN` overrides, so the new
      declared `make_jobs` policy is the only local parallelism control path.
- [x] Repo-owned package realization logic does not call live wall-clock or RNG
      APIs; the remaining time references are fixed reproducible epoch constants
      plus tests around them.
- [x] A text-output scan of current store trees found no serialized
      `jobserver-auth` state or emitted `MAKEFLAGS` payloads; the only hits are
      GCC source/header comments that mention the flag syntactically.
- [x] Current store trees contain no live `6.8.0-64-generic` kernel-release
      capture and no Bazel-style workspace-status/stamping markers such as
      `BUILD_TIMESTAMP`, `FORMATTED_DATE`, or `STABLE_*` keys.
- [x] A targeted hostname scan using the live host name `admin` found only
      unrelated source/documentation occurrences, not machine-identity leakage
      emitted by package realization.
- [x] GCC's live stage0 build is already using deterministic per-object
      `-frandom-seed=<object>` values in its internal target-library compiles.
- [x] Fresh glibc stage1 output rebuilt against the corrected GCC stage0 tree
      is clean end to end: `crt1.o` remains remapped and the final
      `/pkgs/store/a73aed7d54bde3ff3372a5f518c1c705-glibc-2.42` payload scan
      contains no residual `buck-out/v2/tmp` or
      `pkgs-configure-make-install` paths.
- [x] The follow-up Glibc stage1 rebuild caused by the new GCC stage0 identity
      published
      `/pkgs/store/b38c5d75fd05af8d1d741d0752df2311-glibc-2.42`
      with epoch `1` mtimes and no final-store scratch-root hits.
- [x] Fresh GCC stage1 output
      `/pkgs/store/7c12e1128b6bd34c6404af8bcac26c09-gcc-15.2.0`
      published with epoch `1` mtimes, no final-store scratch/configure-state
      hits, and clean `libstdc++.a` plus `libstdc++fs.a` scans where the prior
      `d806...` store contained absolute Buck work roots.
- [x] Native GNU grep, GNU awk, and GNU patch are package-definition graph-cut
      behind the sealed imported façade. The repaired GNU awk and GNU grep
      recipes self-host through published `:bin` façades rather than stage0
      inputs where applicable. An isolated export run pinned the repaired
      `b060b888...-gawk`, `d23256b4...-gnugrep`, and
      `f8debe78...-patch` generation; after alias advancement, private
      promotion targets intentionally describe the next publication candidate.
- [x] CPython 3.13.10's installed scratch-root leak was fixed; the zlib-enabled
      Meson-capable `python:build_interpreter` now passes `[reproducible]`,
      `[archive_metadata]`, and its seed boundary with a sealed published root.
- [x] Ordinary `zlib` and `inih` recipes name only canonical package/tool
      labels; substitute and foreign-seed labels are limited to package façade
      aliases plus `bootstrap/tests` boundary assertions.
- [x] Analysis exposed and removed the remaining ordinary graph edge from
      Python/Meson/`inih` through live GNU awk, grep, and patch producers.
      Their canonical labels now analyze as native `store_import` actions,
      and `cquery` filters for `stage|foreign_seed` are empty for Python,
      Meson, `inih`, and `toolchains//:cxx_pkgs`.
- [x] Live bootstrap producer targets and foreign seed wrappers now carry an
      exact visibility allowlist; hydrated substitute targets are visible only
      to canonical public aliases and bootstrap tests. Unpublished bootstrap
      convenience façades are no longer public ordinary APIs.
- [x] The corrected GCC wrapper is now generated by
      `gcc:bin_imported_final` over imported substitute inputs only, pinned as
      `/pkgs/store/bbca89e9477fd196133e0482703d02a3-gcc-wrapper-15.2.0-bin`,
      and exposed to ordinary packages through native `store_import`. The C
      launchers omit GCC runtime RUNPATHs, so `zlib` and `inih` now declare
      only Glibc in their runtime closures.
- [x] A local assembled seventeen-object bundle containing promoted GNU
      grep/awk/patch and the imported-base GCC wrapper replacement passes
      `pkgs_hydrate_store_closure` into an independent disposable store root
      without executing live bootstrap derivations. Its closure manifest is
      byte-for-byte identical to the reviewed pinned closure metadata.
- [x] The live `bootstrap/exports:linux_x86_64_bundle` target is no longer a
      public dependency surface: ordinary targets cannot depend on the
      publication graph, while bootstrap tests can still consume it and a
      publication invocation can select it directly.
- [x] Producer-side seed-check targets and individual object export targets
      are no longer default-public bridges; their visibility is limited to the
      producer, export, and validation surfaces that need them.
- [x] Ordinary execution validation passed after the graph sever: `zlib` plus
      the toolchain smoke targets built through imported aliases, and a fresh
      `inih` Meson build completed after rebuilding ordinary Python and Ninja.
      `readelf` and `pkgs_verify_no_refs` confirm both C-only libraries retain
      only Glibc at runtime and contain no GCC/GCC-wrapper store reference.
      After pinning the repaired text-tool generation, importing its three
      canonical aliases completed in `0.1s` and consumer graph queries remained
      empty for producer/export labels.

## Next Checks

- [x] Imported-store performance: validate the native Buck2
      already-hydrated store import path for ordinary package-toolchain use.
      The source implementation removes the full-tree projection copy and
      redundant materializer re-verification. The native Buck2 importer now
      consumes the pinned source manifest directly, validates its provider
      contract and canonical hash, and constructs its artifact value from one
      store-tree traversal. Imported outputs are content-addressed, and package
      `native_build_inputs` use execution dependency transitions so normal
      packages and package-backed toolchains share imported tool action nodes.
      Targeted Rust tests and analysis-only ordinary graph checks pass. A
      post-transition combined ordinary `zlib` plus C toolchain-smoke execution
      performed one imported GCC validation in `3.9s`, then repeated in `0.0s`.
      A future
      Buck2-compatible directory digest or receipt in substitute metadata can
      avoid or reuse that remaining first-use traversal. The prior two-walk
      implementation's first ordinary `zlib` plus toolchain-smoke run took
      `34.3s`; the fresh `inih` execution run took `9:58.7`, dominated
      separately by native Python and Ninja rebuilds invalidated by corrected
      package identities.
- [ ] Random seeds: keep watching for packages that do not self-seed the way GCC
      already does when profile or coverage-style outputs are introduced.
- [ ] Parallelism stress: compare representative package outputs at
      `make_jobs = 1` versus the fixed default `make_jobs = 64` to catch
      upstream dependency races. Host CPU availability is no longer an implicit
      graph input, but concurrency-sensitive package logic can still be broken
      on its own terms.
- [ ] Archive order and metadata: ratchet the check from sampling to targeted
      scans on freshly rebuilt outputs that emit `.a`, `.tar*`, or `.gz`.
- [ ] Host discovery leaks: inspect published artifacts for host compiler,
      hostname, uname, or `/usr/...` path capture where that information is not
      part of the declared package ABI.
- [ ] Bootstrap seed leakage: distinguish expected stage0 host debug paths from
      regressions that survive into the seed-free final package closures.
- [ ] Expanded pinned closure distribution: publish the locally
      hydrate-verified seventeen-object bundle containing repaired `gnugrep`,
      `gawk`, `patch`, and the imported-base GCC wrapper through an
      authenticated substitute channel, then rerun the Python/Ninja/Meson/
      `inih` gates from a clean consumer against that distributed path.
- [x] Compiler wrapper runtime closure: the wrapper source and semantic builder
      identity stop injecting GCC `RUNPATH` entries into pure C outputs; an
      imported-base wrapper replacement is pinned, and the obsolete GCC
      runtime edges are removed from `zlib` and `inih`.
