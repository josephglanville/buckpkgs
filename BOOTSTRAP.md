# Bootstrap

BuckPkgs should bootstrap to a useful system as fast as possible.

The accepted model is a **static staged DAG**:

```text
foreign Buck2 seed
  -> first BuckPkgs base
    -> toolchain turnover
      -> self-hosted BuckPkgs base
```

The foreign seed stays in the graph. Self-hosting means the final exported
closures no longer reference it; it does **not** mean the seed nodes disappear.

BuckPkgs is not trying to minimize the trust root with a Mes/TinyCC chain.

## Why A Foreign Buck2 Seed

The first tools do not need to arrive as opaque prebuilt tarballs. We can build
them with ordinary Buck2 rules and the already-available Buck2 toolchain.
The current foreign seed wrappers expose:

- shells and basic userland:
  - `bash`
  - `coreutils`
  - `gnused`
  - `gnugrep`
  - `gawk`
  - `gnumake`
  - `gnutar`
  - `gzip`
  - `patch`
  - `diffutils`
  - `findutils`
- toolchain commands:
  - `binutils`
  - `gcc`
- temporary bring-up helpers:
  - `gnum4`
  - `bison`
  - `gettext`/`msgfmt`
  - `python3`

That first set is **foreign** from BuckPkgs' point of view because it was produced
using the pre-existing Buck2 execution/toolchain environment rather than a BuckPkgs
toolchain.

It is still useful:

- it is hermetic to the extent the Buck2 action/toolchain is hermetic
- it is part of the ordinary Buck2 graph
- its outputs are CAS-backed artifacts
- action-cache hits mean it does not rerun unless its declared inputs or action
  definitions change

The foreign seed is therefore a disposable bootstrap prefix, not a second package
manager or a one-off manual install step.

## Canonical Bootstrap Set

When we say "the bootstrap set" without qualification, we mean the final
self-hosted package set that normal BuckPkgs consumers should use after turnover:

- shells and core userland:
  - `bash`
  - `coreutils`
  - `diffutils`
  - `findutils`
- text tools:
  - `gawk`
  - `gnugrep`
  - `gnupatch`
  - `gnused`
- archive and compression tools:
  - `gnutar`
  - `gzip`
  - `bzip2`
- build and toolchain tools:
  - `gnumake`
  - `binutils`
  - `gcc`
  - `glibc`

The bootstrap graph also contains packages needed to construct that set, but not
part of the exported bootstrap contract:

- Linux kernel headers
- GCC support libraries: `gmp`, `mpfr`, and `libmpc`
- currently foreign-only helpers used while bringing up `glibc`: `bison`,
  `gettext`/`msgfmt`, and `python3`

A package can be required to build the bootstrap set without being one of the
initial always-present tools we promise to users. If we later add `xz`,
`pkg-config`, or other useful defaults, update this section deliberately rather
than letting the definition drift through incidental graph edges.

## Static DAG, Not Recursive Evaluation

Compiler turnover does not require recursive package-set evaluation.

Give each generation a distinct node:

```text
foreign_cc
  -> binutils_0
  -> gcc_0
    -> glibc_1
      -> binutils_1
      -> gcc_1
        -> base_tools_1
        -> gcc_2
          -> base_tools_final
```

Conceptually:

- `gcc_0`
  - built by the foreign compiler
  - still tied to the foreign runtime/toolchain world
- `glibc_1`
  - built by `gcc_0`
- `gcc_1`
  - built using the first BuckPkgs-built libc/toolchain layer
- `gcc_2`
  - rebuilt again using the rebuilt BuckPkgs toolchain pieces
- `base_tools_final`
  - rebuilt using the final selected BuckPkgs toolchain

Every edge points from an older generation to a newer one. The public package
labels point at the selected final generation once it is verified. Intermediate
bootstrap labels stay explicit in the bootstrap graph, such as `bin_stage0`,
`bin_stage1`, and `bin_stage2`, so promoting the default label does not mutate
the turnover DAG.

## What "Self-Hosted" Means

There are two separate facts:

1. the foreign seed remains in the Buck2 graph and may remain cached in CAS
2. final exported BuckPkgs closures must not depend on it

That distinction is important.

Keeping the foreign seed nodes is cheap and useful:

- first build: the foreign seed executes
- later builds: Buck2 serves it from action cache / CAS
- changing only later BuckPkgs stages does not force the seed to rerun

Self-hosting is verified by later checks over the final outputs:

- closure metadata must not include foreign-seed store paths
- byte/reference scanning should verify that final outputs do not embed foreign
  paths accidentally

Runtime turnover and "no previous-generation bytes anywhere" are distinct
checks. A rebuilt compiler can already use the BuckPkgs dynamic linker while debug
strings still mention headers from the compiler generation that built it. Keep
those checks separate while deciding whether to strip, remap, or carry debug
information.

The graph remains static. Promotion to "final" is a naming and verification
choice, not a dynamic graph mutation.

## Lessons From nixpkgs

nixpkgs has two relevant Linux stories:

1. the ordinary `stdenv` bootstrap path from prebuilt bootstrap files
2. `minimal-bootstrap`, which reconstructs a system from far smaller roots using
   Mes, TinyCC, and many transitional packages

BuckPkgs should learn from the first story, not copy the second.

The valuable idea is staged replacement of trusted components:

1. start from a foreign seed
2. build a useful first userland
3. build fresh binutils
4. build a transitional compiler
5. build the intended libc
6. rebuild the compiler against the new libc
7. rebuild the exported base tools
8. assert the final exported closure is seed-free

BuckPkgs can use a Buck2-built foreign seed where nixpkgs uses downloaded bootstrap
files. The later staged turnover still exists.

## Proposed BuckPkgs Stages

### Stage -1: Foreign Buck2 Seed

Ordinary Buck2 actions produce the initial userland needed to build package
recipes and the first BuckPkgs bootstrap steps.

This stage may rely on:

- the existing Buck2 C/C++ toolchain
- existing Buck2 execution assumptions
- a non-BuckPkgs shell/tool universe

Those dependencies are explicitly tolerated only in the bootstrap prefix.

### Stage 0: First BuckPkgs Base

Using the foreign seed, build first `/pkgs/store` versions of:

- `bash`
- `gnused`
- `gnugrep`
- `gawk`
- `coreutils`
- `gnumake`
- `gnutar`
- `gzip`
- `patch`
- `findutils`
- `diffutils`

This stage is already useful for tests and tasks, even before the system is
fully self-hosted.

### Stage 1: Toolchain Turnover

Build explicit generations of:

- fresh `binutils`
- transitional `gcc`
- target libc
- rebuilt `gcc`

The exact number of compiler generations can start close to nixpkgs until the
closure behavior is understood.

For GCC turnover, the first useful split is:

- a raw transitional compiler, which may still be linked to the previous host
  runtime
- a wrapper view that forces BuckPkgs `ld`, headers, crt objects, dynamic linker, and
  runtime search paths for the programs it builds
- a later raw compiler rebuilt through that wrapper, which should itself link
  against BuckPkgs libc

nixpkgs disables GCC's `libcc1` GDB plugin during Linux bootstrap. BuckPkgs should
do the same for turnover outputs unless there is a concrete need for that plugin:
it is not needed for ordinary compilation, and keeping it enabled can retain a
reference chain to the compiler that built GCC.

### Stage 2: Self-Hosted Base

Rebuild the exported bootstrap set with the selected rebuilt BuckPkgs toolchain.
The exact contents are defined by the canonical bootstrap set above rather than
an informal category list.

The acceptance condition is that the exported closure has no references to the
foreign Buck2 seed or foreign toolchain outputs.

## CAS And Reuse

Each bootstrap node is an ordinary Buck2 action with ordinary CAS-backed inputs
and outputs.

That gives us:

- seed outputs cached by normal action keys
- no rerun of the foreign seed unless its declared inputs or actions change
- reuse across later builds and later stages
- a natural path to RE CAS once remote execution is added

CAS persistence is an implementation convenience. It does not weaken the
self-hosting requirement for final exported closures.

## Useful Before Pure

BuckPkgs can expose the Stage 0 useful base while Stage 2 self-hosting is still being
built out. The two should be named distinctly so users can tell whether they are
consuming bootstrap-derived or self-hosted packages.

## RE Friendly

The bootstrap graph must be expressible as normal Buck2 actions so:

- inputs upload to RE CAS
- outputs land in RE CAS
- action-cache hits work across machines
- remote workers receive only the store-path closure needed by each action

## Questions Still Worth Answering

1. What exact foreign-seed contents minimize time-to-useful without dragging in
   unnecessary baggage?
2. How many explicit GCC/libc generations do we actually need before final
   closure checks pass reliably?
3. Which assertions should BuckPkgs provide for "no foreign references":
   - metadata-only closure checks
   - byte scanning for embedded paths
   - both
4. Do we require the first exported `bash`/tool slice to be self-hosted, or is a
   clearly named bootstrap namespace acceptable while the self-hosted closure is
   still being built?
