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

In Buck2, the cleaner shape is structured provider data. The package exports
search roots; the dependency edge supplies the role:

```text
PkgConfigInfo(
  search_paths = [...],
)
```

The standard builders now turn declared dependencies into:

```text
PKG_CONFIG_PATH
PKG_CONFIG_LIBDIR
PKG_CONFIG_PATH_FOR_BUILD
PKG_CONFIG_LIBDIR_FOR_BUILD
PKG_CONFIG_PATH_FOR_TARGET
PKG_CONFIG_LIBDIR_FOR_TARGET
```

for the action being executed. `build_inputs` and `link_inputs` supply the
ordinary host lookup path, `native_build_inputs` supply the build lookup path,
and `target_inputs` supply the target lookup path. Setting `PKG_CONFIG_LIBDIR`
alongside `PKG_CONFIG_PATH` keeps host defaults out of hermetic actions.

That keeps dependency visibility explicit and avoids ambient shell mutation as
the primary model.

## Likely BuckPkgs Surface

For packages that produce pkg-config metadata:

```python
pkg_config_paths = ["lib/pkgconfig"]
```

For a named output split from the package's primary realization:

```python
split_pkg_config_paths = {
    "dev": ["lib/pkgconfig"],
}
```

For consumers:

```python
native_build_inputs = [
    "//development/tools/pkg-config:bin",
]
build_inputs = [
    "//development/libraries/zlib:dev",
]
link_inputs = [
    "//development/libraries/zlib:out",
]
```

The builder collects `PkgConfigInfo` from declared dependencies and
sets the environment for actions that need pkg-config. `pkgconf` is packaged as
the normal native `pkg-config` frontend with `:bin` and `:dev` outputs; the
structured action environment supplies the wrapper semantics without shell
hooks. Documentation-only named outputs are not part of the default package
surface.

## Implemented Surface

- `PkgConfigInfo(search_paths = [...])` is exported by built, projected,
  imported, hydrated, and CAS-backed package outputs.
- `pkgs_make_install_package`, `pkgs_configure_make_install_package`, and
  `pkgs_meson_install_package` lower provider roots into hermetic role-specific
  environment variables.
- `//development/tools/pkg-config:bin` supplies `pkg-config` through a native
  `pkgconf` installation.
- `//development/libraries/zlib:dev` is the first split metadata provider;
  Python consumes it through `pkg-config` while linking against
  `zlib:out`.

This is one of the first places where BuckPkgs should deliberately keep nixpkgs'
package semantics while changing the implementation model.
