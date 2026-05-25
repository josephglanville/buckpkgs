# Packaging

This document is the authoritative package-authoring contract for BuckPkgs.
Design documents explain architecture, and bootstrap/reproducibility documents
record migrations and evidence; new package decisions should be made against
this document.

## Core Rule

Package what an upstream component actually exposes for use, not merely what
today's small package graph happens to consume.

When porting a package:

1. Inspect its installed tree and build configuration.
2. Check the corresponding nixpkgs recipe when available, especially its
   outputs, fixups, feature flags, and dependency roles.
3. Classify each real interface under BuckPkgs' output and dependency model.
4. Select only the payload belonging to those interfaces.
5. Add a cheap non-bootstrap validation target before relying on the package in
   a large consumer or bootstrap rebuild.

Nixpkgs is a high-quality reference corpus, not a naming scheme to copy
blindly. BuckPkgs deliberately differs by focusing its default surface on code
and code-consumption interfaces rather than publishing manuals and
documentation by default.

## Output Roles

Use role-specific public output labels.

| Role | Meaning | Typical payload |
| --- | --- | --- |
| `bin` | Runnable programs and tool-use payload | `bin/`, executable-private resources needed when the tool runs |
| `lib` | Runtime library interface | Versioned shared objects, loaders, loadable runtime modules |
| `dev` | Dynamic-development interface | Headers, CRT objects, linker scripts, dynamic link-name symlinks, `*.pc` metadata |
| `static` | Optional static-link interface | Independently consumable existing static archives and their static-only metadata |
| `out` | Exceptional compound interface | Payload that cannot yet be separated without misrepresenting required behavior |

`static` exposes static libraries emitted by the selected realization. It does
not implicitly request a second static-only build. Enabling archives that the
ordinary realization does not produce is a deliberate recipe/identity change
and requires an explicit justification.

Do not decide an interface is absent just because no current BuckPkgs consumer
uses it. For example, `libpkgconf.a` is a real library API even if the current
graph mostly executes `pkgconf`; it is published as `pkgconf:static`.

## Selective Payloads

Prefer explicit selected outputs over a blanket install output:

- Use `output_paths` for paths belonging to the primary role.
- Use `split_outputs` for real additional roles from the same realization.
- Use `pkgs_package_output` to publish the named split outputs.
- Use `excluded_file_suffixes` for incidental files under an otherwise
  required selected subtree.

Do not publish `man`, `doc`, or `info` outputs by default. Do not permit
documentation to fall into `bin`, `lib`, or `dev` through a broad catch-all
selection.

Do not remove all of `share/` mechanically. Some tools require installed data
to perform their executable contract:

| Package | Required non-code payload | Classification |
| --- | --- | --- |
| `gawk:bin` | `share/awk` | Tool-use data |
| `bison:bin` | `share/bison` skeletons, stylesheets, and templates | Tool-use data; incidental `README.md` is excluded |
| `pkgconf:bin` | `share/aclocal/pkg.m4` | Autoconf tool-use data |
| `glibc:lib` | Selected locale, `gconv`, and NSS runtime modules | Runtime behavior |
| `postgresql` | Selected installed server/runtime data | Runtime behavior |

Every retained broad data path must have a behavioral justification and an
output-policy validation allowance where required.

## Libraries And Projections

For a normal shared library, publish:

| Output | Contents |
| --- | --- |
| `lib` | `libfoo.so.*` and required runtime modules |
| `dev` | Headers, `libfoo.so` link-name projection, pkg-config metadata, and other dynamic-link interface files |
| `static` | `libfoo.a` only when it is a real requested static interface |

Use reference-backed `dev` projections by default. The dynamic link-name
symlink in `dev` should refer to the sibling versioned runtime library in
`lib`, rather than duplicating runtime bytes into `dev`.

Development support archives may remain in `dev` only when the dynamic
development interface itself requires them. PostgreSQL PGXS is the established
example: internal `libpgcommon{,_shlib}.a` and `libpgport{,_shlib}.a` support
installed extension builds. This does not justify publishing unrelated
standalone static client archives in `dev`.

Linker scripts placed in a split `dev` sysroot must refer to sibling runtime
outputs in a form that continues to work under `ld --sysroot`, normally
relative sibling-output paths.

## Dependency Roles

Dependency roles describe why an output needs another output. Do not use
runtime closure as a catch-all for references or build tools.

| Attribute | Use it for |
| --- | --- |
| `native_build_inputs` | Programs executed to realize the package |
| `build_inputs` | Declared development/metadata inputs consumed while configuring or building |
| `target_inputs` | Reserved cross/toolchain role for target-machine headers, libraries, sysroots, CRT objects, and ABI material; it must materially affect emitted actions before ordinary use |
| `link_inputs` | Runtime library interfaces used to link produced artifacts; contributes link lookup and installed runtime search behavior |
| `link_interface_inputs` | Link-time development interfaces required to build, without adding their development trees to installed runtime closure |
| `runtime_inputs` | Objects required to execute the published output |
| `tool_inputs` | Objects required when the published output is itself used as a tool |
| `reference_inputs` | Objects named by payload bytes/symlinks but not otherwise runtime/tool requirements |
| `split_runtime_inputs`, `split_tool_inputs`, `split_reference_inputs` | Per-output forms of the same roles |

For sibling outputs of one realization, use
`split_runtime_outputs`, `split_tool_outputs`, or
`split_reference_outputs` according to the same definitions.
`split_reference_paths` and `split_reference_output_paths` construct selected
reference-backed paths into a declared output.

Examples:

- An executable that dynamically loads its package's `lib` output declares a
  runtime edge from `bin` to `lib`.
- `bison:bin` embeds and invokes GNU m4 while generating parsers; GNU m4 is a
  tool-use dependency, not a runtime library.
- A `dev` link-name symlink into `lib` is an object/reference edge, not a
  reason to advertise the runtime library as a runtime dependency of headers.
- A C-only binary must not inherit GCC C++ runtime interfaces from the compiler
  wrapper. C++ consumers declare required `libstdcxx` and `libgcc`
  interfaces explicitly.

## Pkg-Config Metadata

Package metadata discovery is structured provider data, not ambient host
lookup or shell hooks.

Outputs that provide metadata export search roots:

```python
pkg_config_paths = ["lib/pkgconfig"]
```

Metadata in a split output is declared on that role:

```python
split_pkg_config_paths = {
    "dev": ["lib/pkgconfig"],
}
```

Consumers declare the frontend, metadata interface, and runtime/link
interface separately:

```python
native_build_inputs = [
    "//development/tools/pkg-config:bin",
]
build_inputs = [
    "//development/libraries/zlib:dev",
]
link_inputs = [
    "//development/libraries/zlib:lib",
]
```

Standard builders derive role-specific `PKG_CONFIG_PATH` and
`PKG_CONFIG_LIBDIR` values from declared `PkgConfigInfo` roots, including
empty values when no roots are declared. Host default metadata search paths
must not enter a hermetic package action implicitly.

When installed metadata is moved into a split output, ordinary directory
variable repair is applied as part of realization. A package requiring its
installed `prefix` and associated libtool metadata to move to the split
interface must opt into the identity-bearing
`relocate_split_metadata_prefix = True` behavior. This is exceptional, not a
blanket rewrite policy.

`pkgconf` demonstrates the rule:

- `pkgconf:bin` supplies the frontend and `share/aclocal/pkg.m4`.
- This build emits only static `libpkgconf`; its library API is therefore
  `pkgconf:static`, not `pkgconf:dev`.
- `pkgconf:static` opts into metadata-prefix relocation because its `*.pc` and
  `*.la` files are consumed from the static interface.
- The static archive records pkgconf's configured default search locations in
  `pkgconf:bin`, so `static` declares that sibling reference rather than
  hiding or deleting the API.

## Identity And Fixups

Any declared policy capable of changing installed bytes or closure semantics
must be part of package identity. Existing rule support includes output
selection, split output policy, dependency-role edges, metadata search roots,
excluded suffixes, debug preservation, and split metadata-prefix relocation.

Published store outputs are immutable and sealed. Do not repair an existing
store object on access. When a fixup or output contract changes, change
identity and realize a new output.

Normal code-bearing outputs strip ELF/archive debug metadata by default.
`preserve_debug = True` is a reviewed identity-bearing exception; do not add
compiler/build-tool closure merely to satisfy debug-only references.

Installed payloads must not retain transient scratch paths, checkout paths,
ambient `/usr/local` discovery, undeclared host-tool paths, or unstable archive
metadata. Configure files, `*.pc`, `*.la`, Makefiles, scripts, archives, and
driver binaries are all possible leakage surfaces.

## Bootstrap Boundary

Ordinary packages consume pinned imported bootstrap outputs and must not
depend on live bootstrap producers, foreign seed targets, or
`bootstrap/exports` turnover targets.

Changes to an ordinary package or to a default-off builder feature do not by
themselves require rebuilding/publishing the bootstrap generation. A new
bootstrap generation is required when a live exported bootstrap package
contract changes, a new policy is enabled on one of those outputs, or the
published closure is intentionally replaced.

## Established Interfaces

These classifications are settled unless new upstream behavior or a declared
consumer proves they need revision:

| Package | Interfaces and exceptions |
| --- | --- |
| `zlib`, `inih`, `lz4`, `zstd`, `ncurses`, `readline` | `lib` plus reference-backed `dev`; no static projection merely because upstream may emit archives |
| `libcap` | Dynamic `lib`/`dev` only in the selected build, matching removal of unrequested static archives |
| `bubblewrap` | `bin` linked against `libcap:lib` |
| `pkgconf` | `bin` plus actual static library API in `static`; `pkg.m4` remains with the tool |
| `bison` | `bin` plus required parser-generation data; GNU m4 is tool-use |
| `perl` | Selected interpreter/runtime outputs; embedding headers/archive are not in its native-tool surface |
| `postgresql` | `lib`, `bin`, and `dev`; PGXS-specific support archives/tools remain on `dev` only |
| `linux-headers` | `dev` only |
| `glibc` | `lib` runtime feature contract plus `dev` headers/CRT/link interface and dynamic-link support archive; full static archives only as requested `static` |
| `gcc` | Compiler `bin`/`dev` plus distinct `libgcc` and `libstdcxx` interfaces; do not impose C++ runtime on C consumers |

## Porting Checklist

Before editing a recipe:

- [ ] Inspect upstream installed payload and enabled features.
- [ ] Check nixpkgs for established interfaces, output split, fixups, and
      dependencies where a recipe exists.
- [ ] Classify interfaces by upstream behavior rather than current graph use.
- [ ] Choose explicit `bin`/`lib`/`dev`/`static` roles and document any `out`
      exception.
- [ ] Select required paths; exclude documentation/incidental files without
      deleting required tool/runtime data.
- [ ] Declare runtime, tool-use, link-interface, and reference roles
      independently.
- [ ] Declare pkg-config roots and any narrowly justified split-metadata
      relocation.
- [ ] Ensure every byte-affecting policy is identity-bearing.

Before relying on the package:

- [ ] Add or reuse a cheap non-bootstrap validated output.
- [ ] Validate default-output payload policy and declared store references.
- [ ] Run `pkg-config` or executable behavior probes for exported interfaces
      that claim them.
- [ ] Inspect produced files for prohibited documentation, transient paths,
      undeclared references, and runtime ownership.
- [ ] Run `[reproducible]`, `[archive_metadata]`, seed-free, ELF, and
      graph-boundary checks where applicable.
- [ ] Validate at least one real downstream consumer before allowing a package
      contract change to motivate an expensive rebuild.

Representative cheap gates already used by this repository include:

```text
root//validation:projected_contract_dev_bundle
root//validation:split_cc_wrapper_contract
root//validation:payload_policy_contract
root//validation:elf_debug_strip_contract
root//validation:elf_runtime_contract
root//validation:elf_cxx_runtime_contract
root//bootstrap/tests:ordinary_pkgconf_bin
root//bootstrap/tests:ordinary_pkgconf_static
root//bootstrap/tests:ordinary_bison_bin
root//validation:bubblewrap_runtime_integration
```
