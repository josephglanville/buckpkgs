load("@prelude//toolchains:cxx.bzl", "system_cxx_toolchain")
load("@prelude//toolchains:python.bzl", "system_python_bootstrap_toolchain")
load("@prelude//toolchains:rust.bzl", "system_rust_toolchain")
load("@root//rules:toolchains.bzl", "pkgs_gcc_cxx_toolchain")

def buckpkgs_toolchains():
    # Keep the ambient CXX toolchain bootstrap-safe. Package-backed GCC remains
    # an explicit choice even when it is backed by finalized native imports.
    system_cxx_toolchain(
        name = "cxx",
        compiler = "gcc",
        compiler_type = "gcc",
        cxx_compiler = "g++",
        linker = "g++",
        visibility = ["PUBLIC"],
    )

    system_cxx_toolchain(
        name = "cxx_bootstrap",
        compiler = "gcc",
        compiler_type = "gcc",
        cxx_compiler = "g++",
        linker = "g++",
        visibility = ["PUBLIC"],
    )

    pkgs_gcc_cxx_toolchain(
        name = "cxx_pkgs",
        gcc = "root//development/compilers/gcc:bin",
        bintools = "root//development/tools/misc/binutils:bin",
        visibility = ["PUBLIC"],
    )

    system_python_bootstrap_toolchain(
        name = "python_bootstrap",
        visibility = ["PUBLIC"],
    )

    # Rust remains a bootstrap toolchain while BuckPkgs is still built by
    # repo-local Rust helper binaries. Other default toolchains should enter
    # this cell only once they have package-backed implementations or a live
    # bootstrap consumer in this repository.
    system_rust_toolchain(
        name = "rust",
        default_edition = "2021",
        visibility = ["PUBLIC"],
    )
