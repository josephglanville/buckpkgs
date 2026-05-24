load("//rules:pkgs.bzl", "PkgsPackageInfo")

def _pkgs_executable_tool_impl(ctx):
    package = ctx.attrs.package[PkgsPackageInfo]
    executable = "{}/{}".format(package.logical_store_path, ctx.attrs.path)

    return [
        DefaultInfo(),
        RunInfo(
            args = cmd_args(
                executable,
                hidden = package.tool_store_outputs,
            ),
        ),
    ]

_pkgs_executable_tool = rule(
    impl = _pkgs_executable_tool_impl,
    attrs = {
        "package": attrs.dep(providers = [PkgsPackageInfo]),
        "path": attrs.string(),
    },
)

def pkgs_executable_tool(name, package, path, visibility = []):
    _pkgs_executable_tool(
        name = name,
        package = package,
        path = path,
        visibility = visibility,
    )

def pkgs_bootstrap_rust_binary(**kwargs):
    if "_cxx_toolchain" not in kwargs:
        kwargs["_cxx_toolchain"] = "toolchains//:cxx_bootstrap"
    native.rust_binary(**kwargs)

def _tool_target_name(toolchain_name, tool_name):
    return "_{}_{}".format(toolchain_name, tool_name)

def _tool_target_label(toolchain_name, tool_name):
    return ":{}".format(_tool_target_name(toolchain_name, tool_name))

def _pkgs_tool(toolchain_name, tool_name, package, path):
    pkgs_executable_tool(
        name = _tool_target_name(toolchain_name, tool_name),
        package = package,
        path = path,
    )
    return _tool_target_label(toolchain_name, tool_name)

def pkgs_gcc_cxx_toolchain(
        name,
        gcc = "root//development/compilers/gcc:bin",
        bintools = "root//development/tools/misc/binutils:bin",
        visibility = []):
    cc = _pkgs_tool(name, "cc", gcc, "bin/gcc")
    cxx = _pkgs_tool(name, "cxx", gcc, "bin/g++")
    ar = _pkgs_tool(name, "ar", bintools, "bin/ar")
    nm = _pkgs_tool(name, "nm", bintools, "bin/nm")
    objcopy = _pkgs_tool(name, "objcopy", bintools, "bin/objcopy")
    objdump = _pkgs_tool(name, "objdump", bintools, "bin/objdump")
    ranlib = _pkgs_tool(name, "ranlib", bintools, "bin/ranlib")
    strip = _pkgs_tool(name, "strip", bintools, "bin/strip")

    native.cxx_toolchain(
        name = name,
        archiver = ar,
        archiver_supports_argfiles = True,
        archiver_type = "gnu",
        assembler = cc,
        assembler_type = "gcc",
        c_compiler = cc,
        c_compiler_type = "gcc",
        compiler_type = "gcc",
        cpp_dep_tracking_mode = "makefile",
        cxx_compiler = cxx,
        cxx_compiler_type = "gcc",
        generate_linker_maps = False,
        linker = cxx,
        linker_type = "gnu",
        nm = nm,
        objcopy_for_shared_library_interface = objcopy,
        objdump = objdump,
        pic_behavior = "supported",
        ranlib = ranlib,
        strip = strip,
        visibility = visibility,
    )
