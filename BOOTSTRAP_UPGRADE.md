# Bootstrap Output Upgrade Plan

## Status

This document records the desired end state and execution checklist for
normalizing the bootstrap closure. The bootstrap should not be rebuilt or
republished until the pre-build checklist is complete.

The purpose of this gate is simple: package identity changes at this boundary
are expensive. We should settle output semantics, dependency semantics, and
publication mechanics before generating a new canonical bootstrap generation.

The end-state choices in this document are now fixed for the first normalized
generation:

- `bin`, `lib`, `dev`, and optional `static` are the canonical roles.
- `static` is implemented as a role but is not broadly published in the
  initial bootstrap closure. Components required for dynamic application
  linking, such as glibc nonshared objects, remain in `dev`.
- `static` is a selective interface over static-link payload emitted by the
  package's normal realization, such as `libfoo.a`; it does not request a
  separate static-only build. If an upstream recipe does not emit a required
  archive alongside its ordinary outputs, enabling that archive is an
  explicit package identity change, and a second realization requires a
  documented exception.
- Shared library development interfaces are reference-backed by default:
  `dev` contains the link-name symlink and metadata, while versioned runtime
  shared objects stay in `lib`.
- GCC initially projects separate `libgcc` and `libstdcxx` runtime and
  development interfaces from one compiler realization.
- `glibc:lib` supports `C.UTF-8`, required `gconv` and NSS runtime modules,
  and does not depend on ambient loader-cache or preload configuration. It
  does not propagate final `libgcc_s`, because final GCC is built after
  glibc; consumers that require unwinding carry an explicit `gcc:libgcc`
  runtime dependency.
- Command tools retain exactly the non-`bin/` runtime paths listed in
  [Bootstrap Command Tools](#bootstrap-command-tools); no undocumented
  `share/`, manual, info, or documentation payload is accepted.
- Code-bearing outputs discard ELF/archive debug metadata by default. A
  package that intentionally preserves or later projects debug information
  must request that as an explicit output-policy exception; build-tool debug
  paths are not runtime or reference dependencies of `bin`, `lib`, or `dev`.

## Design Goals

- Ordinary builds consume finalized substitutes and never implicitly enter the
  live bootstrap turnover graph.
- Outputs describe consumption roles precisely: execution, runtime libraries,
  development interfaces, or static linking.
- Default outputs contain executable code and required runtime/build
  interfaces, not manuals, info pages, documentation, examples, or incidental
  installation payload.
- Store references, runtime requirements, and build-tool requirements are not
  conflated.
- A normalized live closure is built once, inspected, published, pinned, and
  then validated through imported ordinary consumers.

## Output Contract

Bootstrap and ordinary packages should use these roles consistently:

| Output | Meaning | Payload policy |
| --- | --- | --- |
| `bin` | Executable/tool payload | Programs plus tool-private resources required to execute or use the tool |
| `lib` | Runtime library payload | Shared libraries, loaders, and loadable runtime modules |
| `dev` | Dynamic-development interface | Headers, CRT files, linker scripts, link-name symlinks, pkg-config data, and other build metadata |
| `static` | Optional static-link interface | Existing static archives and static-only metadata selected from the normal realization when required |

`out` should remain only for a deliberately compound payload that cannot yet
be split without misrepresenting required dependency behavior. It is not the
default label for normalized foundational packages.

Do not publish `man`, `doc`, or `info` outputs by default. Such payloads are
opt-in exceptions, not part of the bootstrap base.

Do not preserve debug sections in code-bearing outputs by default. A future
`debug` role may carry separated debug information when a declared consumer
needs it; until then, `preserve_debug = True` is a reviewed exception rather
than a reason to retain build compilers in ordinary object closures.

Runtime data may remain in a code-bearing output when its use is part of the
declared behavior of that package. Examples include:

- `share/awk` for installed awk programs used by `gawk`
- glibc loader/runtime modules such as `gconv` data or NSS modules if retained
  by the selected libc feature contract

Development support archives may remain in `dev` only when they are required
to use that development interface rather than offered as a generic static-link
product. PostgreSQL's PGXS/pkg-config interface requires its internal
`libpgcommon{,_shlib}.a` and `libpgport{,_shlib}.a` archives; standalone
static client library archives are not selected.

## Dependency Model Upgrade

The current substitute/provider model derives exported store references from
runtime closure. That is insufficient once development outputs refer to
runtime objects without themselves being runtime roots.

Before bootstrap output normalization, the package/provider/manifest model
must distinguish:

| Concept | Meaning |
| --- | --- |
| `references` / object closure | Other store objects named in bytes, symlinks, scripts, linker metadata, or provider-defined projections |
| `runtime_closure` | Store objects required to execute this package output |
| build or tool-use closure | Store objects required when this output is used as a compiler, linker, or build tool |

Required properties:

- A `dev` output may reference a `lib` output without advertising the full
  referenced tree as its own runtime dependency.
- Builders distinguish link interfaces from linked runtime payloads:
  `link_interface_inputs` contributes development search paths such as
  `dev/lib` without adding RUNPATH or runtime closure, while `link_inputs`
  supplies runtime-library search paths and installed RUNPATH.
- A compiler wrapper may carry the tool-use closure required to compile/link
  while produced C programs carry only their actual runtime libraries.
- Closure export and hydration validate all object references, independently
  of runtime dependency propagation.
- Canonical imported-provider declarations preserve the same reference and
  runtime semantics as live outputs.

This is a prerequisite for a stable fine-grained bootstrap closure, not an
optional cleanup.

## Projection Policy

Development outputs should compose over runtime outputs rather than duplicate
runtime library bytes by default.

Preferred pattern:

| Output | Example contents |
| --- | --- |
| `lib` | `libfoo.so.*`, runtime modules |
| `dev` | `include/`, `libfoo.so` link-name symlink into `lib`, `*.pc`, linker scripts |
| `static` | `libfoo.a` |

A projection mechanism may create symlink-backed or otherwise
reference-backed development interfaces from the same realization. A copied
projection remains useful only for a documented exception where a consuming
tool cannot operate through separate referenced outputs.

Do not make full copied `lib/` directories in `dev` the foundational default:
that would create a second linkable runtime location, enlarge substitutes, and
risk recording RUNPATHs against development outputs.

## Foundational Libraries

### GMP, MPFR, And libmpc

| Package | Outputs | Contract |
| --- | --- | --- |
| `gmp` | `lib`, `dev`, optional `static` | Runtime shared objects in `lib`; headers and dynamic link interface in `dev`; static archives only when requested |
| `mpfr` | `lib`, `dev`, optional `static` | `dev` consumes `gmp:dev`; runtime `lib` depends on `gmp:lib` |
| `libmpc` | `lib`, `dev`, optional `static` | `dev` consumes GMP/MPFR `dev`; runtime `lib` depends on GMP/MPFR `lib` |

GCC configuration consumes these libraries through their development
interfaces. The installed GCC executables declare only the corresponding
runtime library outputs needed to run compiler frontends.

### Linux Headers

`linux-headers` exposes `dev` only. It contains kernel headers and no runtime
payload.

### Glibc

Glibc requires an explicit contract before publication:

| Output | Contract |
| --- | --- |
| `lib` | Loader, shared runtime libraries, and approved runtime modules/data |
| `dev` | Headers, CRT files, linker scripts, link-name symlinks, and `libc_nonshared.a`, which is required for dynamic application linking |
| `static` | Full static-link archives when requested |
| `bin` | User-facing libc tools only if intentionally exported later; not part of the minimal compiler/runtime bootstrap by default |

Before finalizing `glibc:lib`, choose and document:

- Whether the default runtime supports only `C`/`POSIX`, or includes a minimal
  `C.UTF-8` locale contract.
- Whether `gconv` modules are part of the default runtime interface.
- Which NSS/loadable runtime modules are included.
- How ambient `/etc/ld.so.cache` and preload behavior are avoided or
  explicitly controlled.
- How glibc's dynamic loading of `libgcc_s` is represented without introducing
  a glibc-to-final-GCC bootstrap cycle: the initial normalized generation does
  not propagate final `gcc:libgcc` from `glibc:lib`; consumers requiring
  unwinding carry that runtime edge explicitly.

The minimum output must be behaviorally complete for the intended bootstrap
and ordinary consumer tests; minimizing bytes alone is not sufficient.

## Compiler And Linker Tools

### Binutils

| Output | Contract |
| --- | --- |
| `bin` | Executables and target-specific linker scripts/resources required by those tools |
| `dev` | Plugin/API headers or build metadata only when a declared consumer needs them |
| `lib` | Binutils shared libraries only if built and required as reusable runtime interfaces |

Manuals, info pages, locales, and unconsumed development archives are not
included in the bootstrap `bin` output.

### GCC

GCC should not remain one compound public runtime interface. A single GCC
realization may initially produce several outputs, but ordinary dependencies
must select only their actual role:

| Output/interface | Contract |
| --- | --- |
| compiler `bin` | Drivers, frontends, `libexec`, compiler-private resources required to run GCC |
| compiler `dev` | C++ headers, plugin/include development interface, and other compilation metadata |
| compiler runtime `lib` | Target runtime shared libraries such as `libgcc_s` and `libstdc++` required by generated executables |
| optional `static` | Static compiler-runtime libraries only where required |

Separately named `libgcc` and `libstdc++` interfaces are preferred. They may
initially be projected from the GCC realization rather than built as separate
upstream packages.

GCC private built-in headers, CRT objects, and static support archives used
unconditionally by the compiler driver remain private resources of compiler
`bin`; `static` means a separately consumable static-library interface, not
splitting resources required merely to make the compiler executable useful.
Installed `fixincludes`/header-install helpers are not part of the published
compiler output, and plugin development data is projected into `dev`.

Consequences:

- C-only outputs depend on `glibc:lib` unless source or ELF inspection proves
  an additional runtime.
- C++ outputs depend explicitly on the required GCC runtime `lib` interfaces.
- Compiler executable outputs retain their own GMP, MPFR, libmpc, glibc, and
  compiler-runtime execution closure where required.
- A compiler wrapper does not automatically impose compiler runtime libraries
  on every produced executable.

### Compiler Wrapper

The wrapper interface must distinguish:

| Input | Purpose |
| --- | --- |
| compiler `bin` | Actual compiler executable/resource payload |
| bintools `bin` | Assembler/linker tool payload |
| libc `lib` | Loader and runtime library path injected into linked programs |
| libc `dev` sysroot | Headers, CRT objects, and linker-script interface used at link time |
| GCC runtime/development interfaces | Explicit target-runtime and header/library requirements for C++ compilation/linking |

The current single `libc` input used as both runtime path and sysroot should be
retired before normalized bootstrap publication.

## Bootstrap Command Tools

Bootstrap command packages become explicit `bin` outputs retaining only
required executable behavior:

| Package | Retained paths |
| --- | --- |
| `bash` | `bin/` |
| `gnumake` | `bin/` |
| `gnum4` | `bin/` |
| `coreutils` | `bin/` |
| `findutils` | `bin/find`, `bin/xargs` only; database utilities are not part of the bootstrap tool contract |
| `gnused` | `bin/` |
| `gnugrep` | `bin/` |
| `gnupatch` | `bin/` |
| `gawk` | `bin/`, `lib/gawk/`, `libexec/awk/`, `share/awk/` |
| `gzip` | `bin/` |
| `gnutar` | `bin/`, `libexec/` |
| `diffutils` | `bin/` |
| `bzip2` | `bin/` |

Each exception outside `bin/` must be documented as a required runtime
surface. General documentation, manuals, info pages, and locales are excluded
unless specifically adopted as a runtime feature.

All listed command tools are normalized and validated when used in the live
bootstrap graph. The published ordinary compiler/toolchain closure contains
only outputs reached from its selected roots; internal build-only tools such
as `gnum4`, `gzip`, `bzip2`, `gnutar`, or `diffutils` are not added merely
because they were required to produce the generation.

## Automated Policy Validation

Output normalization must be verified mechanically, not by ad hoc spot
inspection alone.

Required checks for the normalized bootstrap closure:

- Reject forbidden default payload paths such as `share/man`, `share/info`,
  documentation trees, and unapproved broad data directories.
- Permit explicitly listed runtime-data exceptions.
- Scan store references in bytes and symlinks and verify all references are
  declared in object closure metadata.
- Strip debug metadata from ELF objects and archives by default so build-tool
  include paths cannot create false runtime/reference closure edges.
- Verify runtime closure separately from reference closure.
- Verify ELF interpreters and RUNPATH/RPATH entries name declared runtime
  outputs only.
- Verify C compiler smoke targets need glibc runtime only.
- Verify C++ compiler smoke targets close over the explicitly exported GCC
  runtime interfaces.
- Retain existing seed-free, reproducibility, sealed-mode, and graph-boundary
  checks.

The live export bundle must not be eligible for publication until these checks
pass.

## Live Closure Migration

Once the model and validation infrastructure are complete, migrate the live
bootstrap producer graph in one coherent change set:

1. Implement independent reference and runtime closure semantics.
2. Implement reference-backed development projections and any explicitly requested static projection.
3. Update the compiler wrapper and toolchain interface.
4. Normalize GMP, MPFR, libmpc, linux-headers, and glibc outputs.
5. Normalize GCC and binutils output interfaces.
6. Normalize bootstrap command-tool payloads.
7. Update live producer dependency roles and visibility declarations.
8. Update `bootstrap/exports` to export the normalized live closure directly.

The export surface should include separate foundational interfaces where
identities differ, including:

- `gmp_lib`, `gmp_dev`, and optional `gmp_static`
- `mpfr_lib`, `mpfr_dev`, and optional `mpfr_static`
- `libmpc_lib`, `libmpc_dev`, and optional `libmpc_static`
- `glibc_lib`, `glibc_dev`, and optional `glibc_static`
- `linux_headers_dev`
- GCC compiler, GCC runtime, binutils, wrappers, and command-tool outputs

For the initial normalized publication, final live command tools and wrappers
should be exported directly when that avoids routing the first new generation
through obsolete imported output identities.

## Substitute Publication And Cutover

`bootstrap/substitutes` contains reviewed pinned identities. It must not be
edited optimistically or populated with invented CAS values.

A working promotion path is a pre-build gate, not post-build cleanup. Before
starting the long rebuild, identify and smoke-test how to:

- publish each built store tree to the configured CAS service
- produce or update CAS overlay manifests mechanically
- assemble the normalized closure metadata
- compare generated object/closure metadata with checked-in pinned files
- hydrate or directly import the new closure into an independent store root

The cutover sequence is:

1. Build one normalized live export bundle.
2. Run automated payload/reference/runtime validation.
3. Inspect the generated closure and any approved exceptions.
4. Publish every new store object through the approved CAS path.
5. Generate and review pinned substitute metadata.
6. Switch canonical aliases to imported `bin`, `lib`, `dev`, and optional
   `static` outputs.
7. Validate representative ordinary C, C++, Meson, PostgreSQL, and Bubblewrap
   consumers through the imported boundary.

## Ordinary Package Alignment

The canonical imported-surface switch must include packages whose public
contracts already rely on the old broad bootstrap outputs:

- Convert `zlib` and `inih` from broad runtime naming to `lib` plus `dev`
  interfaces.
- Replace generic `glibc:out` consumption with `glibc:lib` for runtime and
  `glibc:dev` for sysroot/development use.
- Remove GCC runtime dependencies from C-only packages after ELF/runtime
  validation proves they are unnecessary.
- Give C++ consumers explicit GCC runtime-library dependencies.
- Preserve the sealed imported bootstrap boundary: ordinary consumers must not
  reach live producer labels after cutover.

This alignment may be implemented after the live producer code is updated, but
must be complete before canonical imported aliases are considered finished.

## Cheap Pre-Migration Proofs

Every infrastructure change must be exercised outside the live bootstrap
producer graph before it is applied to a foundational package:

| Change | Cheap non-bootstrap validation target | What it establishes |
| --- | --- | --- |
| Separate object-reference, runtime, and tool-use closures | `cargo test -p pkgs-tool store_manifests_separate_reference_runtime_and_tool_use_closures` and `root//validation:projected_contract_dev_bundle` | A `dev` object may reference `lib` without propagating it as runtime, and exported closure metadata accepts the split. |
| Reference-backed `dev` projection and rewritten pkg-config paths | `cargo test -p pkgs-tool splits_development_output_and_repairs_pkg_config_paths` and `root//validation:projected_contract_dev_bundle` | The development symlink points into `lib`, selected payload excludes installed documentation, and metadata points at the projected development path. |
| Split-output GNU linker-script rewriting | `cargo test -p pkgs-tool makes_cross_output_linker_script_references_sysroot_safe` and a live `ld --sysroot=<glibc-dev> -lm` probe | A linker script in `dev` retains local archives and spells sibling `lib` references relatively, so GNU `ld` does not re-root runtime objects under the development sysroot. |
| Default debug stripping for code-bearing outputs | `cargo test -p pkgs-tool strips_debug_store_references_by_default_and_preserves_them_only_when_requested` and `root//validation:elf_debug_strip_contract` | DWARF/debug sections cannot retain undeclared build-tool store paths in normal `bin`/`lib`/`dev` outputs; preserving debug information is explicit. |
| Sibling split-output reference/runtime edges for GCC-style projections | `root//validation:projected_contract_dev_bundle` | A projected `bin` may depend on separate runtime libraries, one projected runtime library may depend on another, and `dev` carries separate link-name symlinks into both runtime outputs. |
| Split wrapper libc/runtime/sysroot/GCC-runtime inputs | `cargo test -p pkgs-tool cc_wrapper_tree` and `root//validation:split_cc_wrapper_contract` | Wrapper generation can distinguish development lookup from runtime RPATH inputs before GCC/glibc are split. |
| Default-output payload policy and explicit runtime-data allowances | `cargo test -p pkgs-tool verify_output_policy` and `root//validation:payload_policy_contract` | A deliberately installed `share/doc` payload is rejected while an allowlisted runtime-data path is accepted. |
| Declared reference validation, including absolute projection symlinks | `cargo test -p pkgs-tool verify_declared_refs` and `root//validation:projected_contract_dev_bundle` | Bytes and symlinks cannot retain undeclared store references. |
| ELF interpreter, RUNPATH/RPATH, and compiler-runtime ownership | `root//validation:elf_runtime_contract` and `root//validation:elf_cxx_runtime_contract` | The C executable forbids `libgcc_s`/`libstdc++` and retains only libc runtime ownership; the C++ executable requires both GCC runtime libraries and their declared runtime provider. |
| CAS publication/import/pinning workflow | Publish `root//validation:cas_smoke_tree_substitute` through Foundry, generate its reviewed CAS overlay/pin record, and build `root//validation:cas_smoke_import_validated`. | A previously absent store object can be uploaded and re-imported through the pinned CAS path without a bootstrap rebuild; the full normalized bundle remains a cutover deliverable. |

The synthetic projection package intentionally installs `share/doc` input
which is not selected into any published output. Its projected runtime and
`dev` outputs are passed through the payload and declared-reference validators;
`root//validation:payload_policy_contract` separately proves an allowlisted
runtime-data exception.

`root//validation:bubblewrap_runtime_integration` is an ordinary downstream
executable check, not a fast infrastructure probe: on a cold graph it builds
Meson's Python interpreter path. Keep it for post-cutover validation rather
than the inner bootstrap-development loop.

Use `/home/jglanville/src/buck2/target/debug/buck2` for these targets until a
release build of the adjacent Buck fork is refreshed. The DotSlash bootstrap
launcher resolves to upstream Buck and does not expose the repository's native
store-output and CAS-import action APIs.

## Checklist

### End-State Decisions

- [x] Adopt `bin`, `lib`, `dev`, and optional `static` as the canonical output roles.
- [x] Decide whether `static` is published for the initial bootstrap generation or exposed only on demand.
- [x] Replace copied shared-library `dev` projections with reference-backed development interfaces by default.
- [x] Define separately named GCC runtime interfaces, at minimum for `libgcc_s` and `libstdc++`.
- [x] Define the glibc runtime feature contract: locale baseline, gconv/NSS modules, loader-cache/preload behavior, and `libgcc_s` handling.
- [x] Define which non-`bin/` runtime data paths are permitted for each bootstrap command tool.

### Provider And Rule Infrastructure

- [x] Separate object/store reference closure from runtime closure in providers and substitute manifests.
- [x] Represent build-tool/use closure where compiler and wrapper behavior needs it.
- [x] Implement reference-backed projections for `dev`; keep `static` unexported in this generation until a declared consumer requires it.
- [x] Update pkg-config/link metadata rewriting so development interfaces point at runtime `lib` outputs without duplicating shared libraries.
- [x] Distinguish link-time development interfaces from installed runtime
      dependencies with `link_interface_inputs`, then validate split-library
      consumers without `dev` RUNPATH or runtime-closure leakage.
- [x] Support sibling projected runtime/reference edges needed for GCC `libgcc` and `libstdcxx`, proven through `root//validation:projected_contract_dev_bundle`.
- [x] Split wrapper runtime libc and development sysroot inputs.
- [x] Rewrite split `dev` linker-script references to sibling runtime outputs
      as relative paths; a live glibc `libm.so`/`libc.so` inspection and
      `ld --sysroot=<glibc-dev> -lm` probe resolve the separate `lib` output.
- [x] Make split final C wrappers search GCC's `dev` link-name interface
      without injecting an unused `libgcc` runtime `RUNPATH`; the final GNU
      Patch validated export links with only its declared glibc runtime.
- [x] Ensure all output policy and dependency-role inputs participate in store identity.
- [x] Strip ELF/archive debug metadata from code-bearing outputs by default, retain an explicit exception knob in package identity, and prove it on `root//validation:elf_debug_strip_contract` before restarting live validation.
- [x] Expose `strip` from the foreign binutils seed as the minimal root build-tool addition required to apply that policy to the first self-hosted compiled stage; do not publish it as a new runtime edge.
- [x] Add unit and integration tests for the new provider, manifest, projection, and wrapper contracts.
- [x] Build the non-bootstrap `root//validation:projected_contract_dev_bundle` and `root//validation:split_cc_wrapper_contract` fixtures before editing live producers.
- [x] Review `rules/pkgs.bzl` and related Starlark for duplication, overly repeated role/projection plumbing, and non-idiomatic structure; factored split-output argument/schema handling and pinned-import provider validation, then reran all cheap fixtures plus analysis of ordinary Configure/Meson/PostgreSQL/Bubblewrap targets.

### Validation Infrastructure

- [x] Add a default-output payload policy validator with explicit runtime-data exceptions, proven by `root//validation:payload_policy_contract`.
- [x] Add declared-reference validation for store paths present in bytes and symlinks, attached to `root//validation:projected_contract_dev_bundle`.
- [x] Add runtime validation for ELF interpreter and RUNPATH/RPATH ownership, proven by `root//validation:elf_runtime_contract` and `root//validation:elf_cxx_runtime_contract`.
- [x] Add C and C++ compiler-use tests that verify their distinct runtime closures and `DT_NEEDED` requirements.
- [x] Retain seed-free, replay reproducibility, archive metadata, sealed-mode, visibility, and imported-boundary tests while adding the new checks.

### Publication Readiness

- [x] Locate or implement the intended CAS upload/promotion command: use Foundry `foundryctl cas upload-tree` to publish a realized store tree and return its REAPI directory digest.
- [x] Smoke-test CAS publication and import on a cheap non-bootstrap store object: `root//validation:cas_smoke_tree_substitute` was uploaded and the previously absent `root//validation:cas_smoke_import_validated` materialized with output-policy/reference validation.
- [x] Automate CAS manifest/closure metadata generation or update sufficiently to avoid hand-entered digests: `pkgs_add_cas_manifest` validates an exported object manifest and writes both its CAS overlay and Starlark pin record from the upload digest.
- [x] Verify the review process for generated versus pinned substitute metadata: the checked-in validation overlay and pin were compared byte-for-byte with `pkgs_add_cas_manifest` output before import.

### Live Bootstrap Migration

- [x] Normalize GMP, MPFR, and libmpc live outputs and dependency roles.
- [x] Normalize linux-headers as `dev`.
- [x] Normalize glibc `lib`/`dev`/optional `static` outputs against the chosen runtime contract.
- [x] Normalize GCC compiler and runtime interfaces.
- [x] Normalize binutils and wrapper outputs.
- [x] Normalize command-tool `bin` payloads and documented runtime-data exceptions.
- [x] Keep final command-tool runtime references explicit: `diff --paginate`
      declares Coreutils `pr`, while final Findutils exports only the
      bootstrap-required `find`/`xargs` tools instead of its database scripts.
- [x] Update live producer visibility, seed-free targets, interpreter checks, and exports.
- [x] Analyze all changed targets before triggering the full bootstrap build.

### Rebuild And Cutover

- [x] Build the normalized live bootstrap closure once.
- [x] Run output-policy, reference, runtime, seed-free, and reproducibility validation over the live bundle.
- [x] Publish the reviewed normalized artifacts to CAS.
- [x] Pin new substitute manifests and closure metadata.
- [x] Redirect canonical aliases and package-backed toolchains to normalized imports.
- [x] Align existing ordinary foundational consumers with the new roles.
- [x] Verify package metadata used by ordinary consumers does not retain
      ambient `/usr/local` search paths; LZ4 must generate `liblz4.pc` with
      its declared logical prefix before PostgreSQL is accepted.
- [x] Remove development-only payload from ordinary code-bearing tool outputs
      unless a declared consumer needs a `dev` projection; Perl used as a
      native interpreter must not retain embedding headers or `libperl.a`.
- [x] Validate ordinary code-bearing tool configuration does not serialize
      undeclared build-tool or header-search store paths; Perl's installed
      configuration records generic probe commands and declared libc paths.
- [x] Keep PostgreSQL runtime outputs free of its compile-interface references
      by emptying the server-side `pg_config` view; retain dependency `dev`
      references only on PostgreSQL's `dev` projection.
- [x] Model installed PostgreSQL PGXS tools as `dev` tool-use dependencies
      (`bison`, `flex`, `perl`, and Coreutils install helpers), without
      introducing them into `bin` or `lib` runtime closures.
- [x] Deduplicate pkg-config roots contributed through overlapping build and
      link-interface roles so generated compile metadata has one declared path
      per development dependency.
- [x] Validate clean imported C/C++, PostgreSQL, Meson, and Bubblewrap paths without live-bootstrap ancestry.
- [x] Update `.memory/PROTOCOL.md`, `.memory/TODO.md`, and bootstrap documentation with the completed contract and any deliberate deferrals.

Progress note: the normalized live closure built and validated as a
24-object role-specific generation, was uploaded to Foundry CAS, and is pinned
under `bootstrap/substitutes/linux_x86_64/` with generated Starlark pins in
`bootstrap/substitutes/linux_x86_64_pins/`. Canonical public aliases now
resolve through those imports. The replay surfaced and fixed required
dependencies for final math/command tools, C wrapper `libgcc` lookup without a
C-runtime leak, and sysroot-safe relative glibc linker-script references.
Imported C/C++ and Bubblewrap gates pass through the normalized aliases;
Python/Meson/`inih` and PostgreSQL graph-boundary queries contain no live
bootstrap or foreign-seed ancestry. PostgreSQL `lib`/`bin`/`dev` and its Perl
tool path pass after Perl runtime configuration stopped serializing glibc
development-probe locations.

## Stop Conditions

Do not start the expensive live bootstrap rebuild while any of the following
remain unresolved:

- Reference closure is still forced to equal runtime closure.
- Development projections still require unexplained duplication of runtime
  shared-library bytes.
- GCC-produced runtime libraries have no explicit output/interface contract.
- The glibc runtime feature and hermeticity policy is unspecified.
- The CAS publication/pinning path has not been smoke-tested.
- Required automated validation cannot yet distinguish an approved runtime-data
  exception from accidental output bloat.
