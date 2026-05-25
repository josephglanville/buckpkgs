# pkg-config

The package-authoring contract for `pkg-config` metadata, `PkgConfigInfo`,
role-specific metadata lookup, and `pkgconf:bin`/`:static` is consolidated in
[PACKAGING.md](./PACKAGING.md#pkg-config-metadata).

This file remains as a compatibility pointer for earlier design references.
The implemented rule surface is in [rules/pkgs.bzl](./rules/pkgs.bzl), and the
ordinary frontend package is in
[development/tools/pkg-config/BUCK](./development/tools/pkg-config/BUCK).
