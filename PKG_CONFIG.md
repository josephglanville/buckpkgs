# pkg-config

`pkg-config` is important enough to design explicitly. It is not just another
tool binary.

## What nixpkgs Does

nixpkgs splits the problem into two packages:

1. `pkg-config-unwrapped`
   - the upstream executable
2. `pkg-config`
   - a wrapper around that executable

The wrapper does three important jobs:

1. exposes the correct wrapped executable for build, host, or target role
2. accumulates dependency `lib/pkgconfig` and `share/pkgconfig` directories into
   role-specific search paths
3. rewrites `PKG_CONFIG_PATH` before invoking the real binary

That behavior is why native-library packages can be consumed without every
recipe hand-writing search paths.

## What BuckPkgs Should Keep

Keep the semantics:

- package outputs may export pkg-config search roots
- pkg-config use is role-aware
- consumers only see pkg-config metadata from declared dependencies
- `*.pc` files stay inside the normal immutable store outputs

This is especially important for libraries. nixpkgs already relies on pkg-config
as a stable cross-package interface, and packages such as OpenSSL intentionally
prefer it over other metadata formats.

## What BuckPkgs Should Not Copy Verbatim

Do not copy the shell-hook mechanism just because nixpkgs uses it.

In Buck2, the cleaner shape is structured provider data:

```text
PkgConfigInfo(
  search_paths_by_role = {
    build: [...],
    host: [...],
    target: [...],
  },
)
```

and a tool wrapper or action helper that turns the selected closure into:

```text
PKG_CONFIG_PATH
PKG_CONFIG_PATH_FOR_BUILD
PKG_CONFIG_PATH_FOR_TARGET
```

for the action being executed.

That keeps dependency visibility explicit and avoids ambient shell mutation as
the primary model.

## Likely BuckPkgs Surface

For packages that produce pkg-config metadata:

```python
pkg_config = {
    "out": [
        "lib/pkgconfig",
        "share/pkgconfig",
    ],
    "dev": [
        "lib/pkgconfig",
    ],
}
```

or, more likely, inferred defaults with explicit overrides where packages route
metadata unusually.

For consumers:

```python
native_build_inputs = [
    "//pkgs/build-support/pkg-config-wrapper:out",
]
build_inputs = [
    "//pkgs/development/libraries/openssl:dev",
]
```

The builder collects `PkgConfigInfo` from the declared dependency closure and
sets the environment for actions that need pkg-config.

## Design Questions

1. Should search roots be inferred from conventional output paths, or always
   declared?
2. Should the public package stay named `pkg-config-wrapper`, or should BuckPkgs
   expose the wrapper as `pkg-config` the way nixpkgs does?
3. Is the wrapper a normal BuckPkgs package, a Buck2 toolchain helper, or both?
4. How should `PKG_CONFIG_LIBDIR` be handled for packages like `ncurses` that
   deliberately generate metadata into a chosen output during their own build?
5. Do we need a parallel `CMakeInfo` later, or can BuckPkgs intentionally prioritize
   pkg-config first for native library discovery?

## Current Recommendation

1. keep `pkg-config-unwrapped` and a wrapped `pkg-config`
2. keep role-aware semantics
3. represent exported search roots as providers, not only shell hooks
4. prioritize pkg-config support before broad native-library work

This is one of the first places where BuckPkgs should deliberately keep nixpkgs'
package semantics while changing the implementation model.
