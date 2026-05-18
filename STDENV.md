# stdenv

In nixpkgs, `stdenv` is much larger than "the C compiler."

It is the default package-building environment that ordinary recipes inherit.

## What Is In It

### 1. A Build API

`stdenv.mkDerivation` is the main constructor used by packages. It provides:

- the dependency-role model
  - `depsBuildBuild`
  - `nativeBuildInputs`
  - `buildInputs`
  - propagated variants of those roles
- output handling
- default builder selection
- default configure flag generation
- hardening policy
- reference-policy plumbing
- metadata and override machinery

This is not just syntax sugar; it is the common contract package recipes target.

### 2. A Shell Build Framework

The generated environment contains `setup.sh` plus default builder scripts.
Together they provide:

- phase orchestration
  - unpack
  - patch
  - configure
  - build
  - check
  - install
  - fixup
- hook registration and execution
- helper functions used by package snippets
- output-variable setup

Most small nixpkgs packages are short because they rely on this framework.

### 3. A Default Tool Closure

For Linux, the ordinary final `stdenv` starts from a common PATH containing tools
such as:

- `coreutils`
- `findutils`
- `diffutils`
- `gnused`
- `gnugrep`
- `gawk`
- `gnutar`
- `gzip`
- `bzip2`
- `gnumake`
- `bash`
- `patch`
- `xz`
- `file`

It also injects:

- the compiler wrapper (`cc`)
- extra native inputs such as `patchelf`
- the autotools config-script updater

This is why many recipes do not spell out every basic command they use.

### 4. A Toolchain Handle

`stdenv` carries:

- `cc`
- shell
- build / host / target platforms
- platform predicates such as `isLinux`, `isDarwin`, and so on

The compiler is important, but it is one field inside the wider environment.

### 5. Default Policy

The default native build inputs include cleanup and policy hooks such as:

- patch shebangs
- strip
- move docs
- compress man pages
- multiple-output handling
- reproducible-build setup
- relative symlink fixing
- prune libtool files

So `stdenv` also encodes what a "normal clean package output" means in nixpkgs.

### 6. Bootstrap Staging

On Linux, `stdenv` is rebuilt through multiple bootstrap stages until the final
environment no longer references the seed bootstrap tools. The final stage wires
in the rebuilt shell, compiler, base tools, and permitted closure.

## What This Means For BuckPkgs

BuckPkgs should probably **not** copy `stdenv` as one monolithic concept.

The useful pieces separate naturally:

1. **builder library**
   - standard phases and helpers
2. **toolchain profile**
   - compiler, binutils, libc, shell, platform tuple
3. **base tool set**
   - core tools made available to standard builders
4. **fixup policy**
   - shebang patching, stripping, output splitting, reproducibility
5. **dependency-role model**
   - build / host / target distinctions

That decomposition is probably better suited to Buck2 than a single giant
`stdenv` object.

## Initial BuckPkgs Recommendation

For the first spike:

- keep a small `gnu_autotools` builder that explicitly depends on a base tool
  profile
- keep role-aware dependency fields
- keep toolchain selection explicit
- model reusable fixups as builder features or structured extensions
- avoid recreating Nix's recursive `stdenv.mkDerivation` world model

The lesson from nixpkgs is that packages want a rich default build environment.
The lesson is not that BuckPkgs must preserve the exact `stdenv` abstraction.
