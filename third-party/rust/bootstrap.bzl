load("@prelude//rust:cargo_buildscript.bzl", _buildscript_run = "buildscript_run")
load("@prelude//rust:cargo_package.bzl", _cargo = "cargo")

_BOOTSTRAP_CXX_TOOLCHAIN = "toolchains//:cxx_bootstrap"

def _with_bootstrap_cxx(kwargs):
    if "_cxx_toolchain" not in kwargs:
        kwargs["_cxx_toolchain"] = _BOOTSTRAP_CXX_TOOLCHAIN
    return kwargs

def _rust_library(**kwargs):
    _cargo.rust_library(**_with_bootstrap_cxx(kwargs))

def _rust_binary(**kwargs):
    _cargo.rust_binary(**_with_bootstrap_cxx(kwargs))

def buildscript_run(**kwargs):
    _buildscript_run(**_with_bootstrap_cxx(kwargs))

bootstrap_cargo = struct(
    rust_binary = _rust_binary,
    rust_library = _rust_library,
)
