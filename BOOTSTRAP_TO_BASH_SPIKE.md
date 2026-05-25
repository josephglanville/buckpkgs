# Bootstrap To Bash Spike

This is the original spike proposal. Its package-label and output-policy
examples are superseded by [PACKAGING.md](./PACKAGING.md).

The first implementation spike should prove one narrow path well:

1. build or provide a foreign Buck2 seed
2. rebuild the ordinary shell-supporting tools from source
3. build a source-built `bash`

This is intentionally smaller than "self-hosted BuckPkgs" and intentionally larger
than a toy `hello` package.

## Why Start Here

The nixpkgs minimal-bootstrap `bash` recipe is useful even though BuckPkgs is not
copying the Mes/TinyCC bootstrap path. Its dependency surface is close to the
real package shape we need to support:

- `coreutils`
- a compiler
- `gnumake`
- `gnupatch`
- `gnused`
- `gnugrep`
- `gnutar`
- `gawk`
- `gzip`
- `diffutils`

BuckPkgs should keep that upper part of the graph and replace the lower trust-root
story with a practical foreign Buck2 seed.

## Proposed First Ladder

### -1. Foreign Buck2 Seed

Start from ordinary Buck2-produced seed tools containing at least:

- `bash`
- `coreutils`
- `gcc`
- `binutils`
- libc
- `make`
- `patch`
- `tar`
- `gzip`
- `bzip2`
- `xz`
- `sed`
- `grep`
- `awk`

The seed exists to make the graph useful quickly. It remains in the static DAG
and can stay cached in CAS, but it is not the final exported base closure.

### 1. First Source-Built Utilities

Build the boring, package-shaped tools first:

- `bzip2`
- `gzip`
- `gnused`
- `gnugrep`
- `gawk`
- `diffutils`
- `coreutils`
- `findutils`
- `gnupatch`
- `gnutar`
- `gnumake`

These packages exercise the standard GNU/autotools builder path, ordinary patch
handling, and basic native tool dependency declaration before we touch GCC
turnover.

### 2. Source-Built Bash

Build `bash` against:

- the BuckPkgs-built utilities above
- the foreign compiler/toolchain for the first pass

That gives us a meaningful first acceptance test:

- the package graph is nontrivial
- the recipes need real patch and hook support
- the output is immediately useful to Buck2 users
- we still avoid the complexity of self-hosting GCC in the first spike

### 3. Later, Replace The Foreign Toolchain

Only after the tool slice works should the next spike start turning over:

- `binutils`
- transitional `gcc`
- libc
- rebuilt `gcc`
- rebuilt base tools

That is where BuckPkgs should begin following the ordinary nixpkgs bootstrap staging
more closely, using explicit compiler generations in a static DAG rather than
recursive package-set evaluation.

## Historical Directory Layout Sketch

The spike proposed Nix-like topic directories, with each upstream package in
its own Buck2 package:

```text
pkgs/
  bootstrap/
    seed/
  shells/
    bash/
  tools/
    compression/
      bzip2/
      gzip/
    misc/
      coreutils/
      findutils/
    text/
      diffutils/
      gawk/
      gnugrep/
      gnupatch/
      gnused/
    archivers/
      gnutar/
  development/
    tools/
      build-managers/
        gnumake/
```

With a `BUCK` file in `pkgs/shells/bash/`, the sketch exported its executable
payload using the proposed role:

```text
//pkgs/shells/bash:bin
```

For a library package, it sketched separate runtime and development interfaces:

```text
//pkgs/development/libraries/openssl:bin
//pkgs/development/libraries/openssl:lib
//pkgs/development/libraries/openssl:dev
```

In this sketch, the directory named the upstream package and the target name
named the BuckPkgs consumption role.

The spike allowed `//pkgs/shells:bash` as a convenience alias from a
`pkgs/shells/BUCK` file, rather than making it the primary ownership model.

## Why Split Packages This Way

The main reason is ownership, not evaluation speed:

- package-local patches and scripts live beside the package recipe
- diffs stay scoped to one upstream package
- future package-set overlays can replace one package cleanly
- a manual port has an obvious home

Buck2 does also get concrete benefits from this layout:

- BUCK-file evaluation is cached per package
- different packages can be loaded concurrently
- a change in `bash` does not invalidate the `gnused` BUCK package

That said, source-directory layout does **not** create more build parallelism by
itself. Action parallelism comes from the dependency graph. Do not split one
logical package into many Buck packages just to chase evaluator parallelism.

## Historical Label Sketch

The spike used direct role-specific output labels as its proposed form:

- `//pkgs/shells/bash:bin`
- `//pkgs/tools/text/gnused:bin`
- `//pkgs/tools/text/gnugrep:bin`
- `//pkgs/development/tools/build-managers/gnumake:bin`
- `//pkgs/development/libraries/openssl:bin`
- `//pkgs/development/libraries/openssl:lib`
- `//pkgs/development/libraries/openssl:dev`

It allowed short aliases where they improved human ergonomics materially:

- `//pkgs/shells:bash`
- `//pkgs/tools:text_tools`
- later perhaps `//pkgs:bootstrap_shell`

In that proposal, aliases aggregated or forwarded rather than containing real
recipes.

## Superseded Output Policy

Current output, dependency, and nixpkgs-reference policy is defined in
[PACKAGING.md](./PACKAGING.md). This section is retained only to show where
the spike originally required that decision.

## What This Spike Should Settle

By the time `bash` builds, we should know:

1. whether TOML can express this slice without becoming code-shaped data
2. whether restricted Starlark is materially cleaner on the same slice
3. what the first real builder surface needs to be
4. what closure metadata a BuckPkgs package target must expose to Buck2 consumers
5. what package-local files belong beside recipes versus inside shared builder
   libraries

That is enough signal to choose the authoring model before expanding toward GCC,
SQLite, or PostgreSQL.
