# Bootstrap Output Upgrade Record

## Status

This document records the completed normalized bootstrap migration and
publication cutover. Current package-authoring policy is defined only in
[PACKAGING.md](./PACKAGING.md); this record is retained for the requirements
that were discharged before the expensive bootstrap rebuild.

The normalized live closure built and validated as a 24-object role-specific
generation, was uploaded to Foundry CAS, and is pinned under
`bootstrap/substitutes/linux_x86_64/` with generated Starlark pins in
`bootstrap/substitutes/linux_x86_64_pins/`. Canonical public aliases now
resolve through those imports.

## Completed Checklist

### End-State Decisions

- [x] Adopt the canonical role model now specified in `PACKAGING.md`.
- [x] Decide whether static interfaces are published in the initial bootstrap generation or exposed only on demand.
- [x] Replace copied shared-library development projections with reference-backed interfaces by default.
- [x] Define separately named GCC runtime interfaces, at minimum for `libgcc_s` and `libstdc++`.
- [x] Define the glibc runtime feature contract: locale baseline, gconv/NSS modules, loader-cache/preload behavior, and `libgcc_s` handling.
- [x] Define approved non-`bin/` runtime-data paths for each bootstrap command tool.

### Provider And Rule Infrastructure

- [x] Separate object/store reference closure from runtime closure in providers and substitute manifests.
- [x] Represent build-tool/use closure where compiler and wrapper behavior needs it.
- [x] Implement reference-backed development projections and keep unneeded static payload unexported.
- [x] Rewrite pkg-config/link metadata for split development/runtime interfaces without duplicating shared library bytes.
- [x] Distinguish link-time development interfaces from installed runtime dependencies with `link_interface_inputs`.
- [x] Support sibling projected runtime/reference edges needed for GCC `libgcc` and `libstdcxx`.
- [x] Split wrapper runtime libc and development sysroot inputs.
- [x] Rewrite split development linker-script references so `ld --sysroot=<glibc-dev> -lm` resolves sibling runtime outputs.
- [x] Make final C wrappers find GCC's link interface without injecting an unused `libgcc` runtime `RUNPATH`.
- [x] Ensure output policy and dependency-role inputs participate in store identity.
- [x] Strip ELF/archive debug metadata by default, retain an identity-bearing exception knob, and prove the behavior on `root//validation:elf_debug_strip_contract`.
- [x] Expose seed `strip` only as the build tool required to apply the initial default debug fixup.
- [x] Add unit and integration tests for provider, manifest, projection, and wrapper contracts.
- [x] Prove new infrastructure on non-bootstrap fixtures before editing live producers.
- [x] Review `rules/pkgs.bzl` and related Starlark for duplication and idiomatic structure.

### Validation Infrastructure

- [x] Add default-output payload-policy validation with explicit runtime-data exceptions.
- [x] Add declared-reference validation for store paths in bytes and symlinks.
- [x] Add ELF interpreter and RUNPATH/RPATH ownership validation.
- [x] Add C and C++ compiler-use tests for distinct runtime closure and `DT_NEEDED` requirements.
- [x] Retain seed-free, replay reproducibility, archive metadata, sealed-mode, visibility, and imported-boundary checks.

### Publication Readiness

- [x] Implement and smoke-test CAS upload/promotion with `foundryctl cas upload-tree`.
- [x] Publish and re-import a cheap non-bootstrap store object through the CAS path.
- [x] Automate CAS manifest and Starlark pin generation with `pkgs_add_cas_manifest`.
- [x] Compare generated validation overlay and pin data byte-for-byte with reviewed pinned files.

### Live Bootstrap Migration

- [x] Normalize GMP, MPFR, libmpc, linux-headers, glibc, GCC, Binutils, wrappers, and command-tool outputs.
- [x] Keep final command-tool runtime references explicit, including Coreutils `pr` for `diff --paginate` and selected Findutils tools only.
- [x] Update live producer visibility, seed-free targets, interpreter checks, and exports.
- [x] Analyze changed targets before triggering the full bootstrap build.

### Rebuild And Cutover

- [x] Build the normalized live bootstrap closure once.
- [x] Run payload-policy, reference, runtime, seed-free, and reproducibility validation over the live bundle.
- [x] Publish reviewed normalized artifacts to CAS and pin substitute manifests and closure metadata.
- [x] Redirect canonical aliases and package-backed toolchains to normalized imports.
- [x] Align ordinary foundational consumers with the new roles.
- [x] Remove ambient `/usr/local` search paths from ordinary metadata, including LZ4 metadata used by PostgreSQL.
- [x] Keep Perl's native-interpreter surface free of embedding headers and `libperl.a`.
- [x] Keep PostgreSQL runtime outputs free of compile-interface references while retaining PGXS requirements on its development output.
- [x] Model installed PostgreSQL PGXS tools as development tool-use dependencies only.
- [x] Correct Bison's executable/data/tool-use contract and exclude its incidental installed README.
- [x] Preserve pkgconf's real static library API and its required tool-use data.
- [x] Make pkgconf's relocated static metadata prefix repair explicit and identity-bearing.
- [x] Deduplicate pkg-config roots in generated compile metadata.
- [x] Validate imported C/C++, Meson, PostgreSQL, and Bubblewrap paths without live-bootstrap ancestry.
- [x] Update durable records with completed decisions and deliberate deferrals.

## Evidence Summary

The migration proved the required contracts through:

- `root//validation:projected_contract_dev_bundle`
- `root//validation:split_cc_wrapper_contract`
- `root//validation:payload_policy_contract`
- `root//validation:elf_debug_strip_contract`
- `root//validation:elf_runtime_contract`
- `root//validation:elf_cxx_runtime_contract`
- `root//validation:cas_smoke_import_validated`
- ordinary imported C/C++, Meson, PostgreSQL, libcap, and Bubblewrap gates
- seed-free and graph-boundary checks for the normalized public surfaces

The replay identified and corrected final math/command-tool dependencies, C
wrapper `libgcc` lookup without a C-runtime leak, sysroot-safe glibc
linker-script references, Perl configuration leakage, Bison output
classification, and pkgconf static-interface metadata.
