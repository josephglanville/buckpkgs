STORE_ABI_VERSION = "pkgs-store-v0"
LOGICAL_STORE_ROOT = "/pkgs/store"

PkgsPackageInfo = provider(
    fields = [
        "build_closure",
        "foreign_runtime_entries",
        "is_foreign",
        "logical_store_path",
        "name",
        "output",
        "realized_stamp",
        "runtime_closure",
        "runtime_realized_stamps",
        "store_entry",
        "store_path_key",
        "tree",
        "version",
    ],
)

def _store_name(name, version, output):
    base = "{}-{}".format(name, version)
    return base if output == "out" else "{}-{}".format(base, output)

def _package_instance_digest(ctx, deps_by_role):
    parts = [
        STORE_ABI_VERSION,
        "name={}".format(ctx.attrs.package_name),
        "version={}".format(ctx.attrs.version),
        "builder={}".format(ctx.attrs.builder),
        "identity={}".format(ctx.attrs.identity),
        "output={}".format(ctx.attrs.output),
    ]

    for role, deps in deps_by_role:
        parts.append("role={}".format(role))
        for dep in deps:
            parts.append(dep.logical_store_path)

    return sha256("\n".join(parts))

def _store_path_parts(ctx, deps_by_role):
    store_name = _store_name(
        ctx.attrs.package_name,
        ctx.attrs.version,
        ctx.attrs.output,
    )
    package_digest = _package_instance_digest(ctx, deps_by_role)
    store_path_key = sha256(
        "\n".join([
            STORE_ABI_VERSION,
            package_digest,
            ctx.attrs.output,
            store_name,
        ]),
    )[:32]
    store_entry = "{}-{}".format(store_path_key, store_name)
    return (store_path_key, store_entry)

def _append_unique(out, seen, entries):
    for entry in entries:
        if entry not in seen:
            seen[entry] = True
            out.append(entry)

def _collect_entries(deps):
    seen = {}
    closure = []

    for dep in deps:
        _append_unique(closure, seen, dep)

    return closure

def _collect_closure(store_entry, deps):
    closure = _collect_entries(deps)
    seen = {entry: True for entry in closure}

    if store_entry not in seen:
        closure.append(store_entry)

    return closure

def _collect_runtime_realized_stamps(store_entry, realized_stamp, deps):
    seen = {}
    entries = []
    stamps = []

    for dep in deps:
        for entry, stamp in zip(dep.runtime_closure, dep.runtime_realized_stamps):
            if entry not in seen:
                seen[entry] = True
                entries.append(entry)
                stamps.append(stamp)

    if store_entry not in seen:
        entries.append(store_entry)
        stamps.append(realized_stamp)

    return stamps

def _deps_by_role(ctx):
    return [
        ("native_build_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]),
        ("build_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]),
        ("target_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.target_inputs]),
        ("runtime_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.runtime_inputs]),
    ]

def _pkgs_package_impl(ctx):
    tree = ctx.attrs.src[DefaultInfo].default_outputs[0]
    deps_by_role = _deps_by_role(ctx)
    store_path_key, store_entry = _store_path_parts(ctx, deps_by_role)
    logical_store_path = "{}/{}".format(LOGICAL_STORE_ROOT, store_entry)
    realized_stamp = ctx.actions.declare_output("{}.realized".format(ctx.label.name))
    build_closure = _collect_closure(
        store_entry,
        [dep.build_closure for _, deps in deps_by_role for dep in deps],
    )
    runtime_deps = [dep for dep in ctx.attrs.runtime_inputs]
    runtime_infos = [dep[PkgsPackageInfo] for dep in runtime_deps]
    runtime_closure = _collect_closure(
        store_entry,
        [dep.runtime_closure for dep in runtime_infos],
    )
    runtime_realized_stamps = _collect_runtime_realized_stamps(
        store_entry,
        realized_stamp,
        runtime_infos,
    )
    foreign_runtime_entries = _collect_entries(
        [dep.foreign_runtime_entries for dep in runtime_infos],
    )
    if ctx.attrs.foreign:
        foreign_runtime_entries = _collect_closure(store_entry, [foreign_runtime_entries])

    ctx.actions.run(
        cmd_args(
            [
                ctx.attrs._realizer[RunInfo],
                "--source",
                tree,
                "--store-root",
                ctx.attrs.realization_root,
                "--store-path",
                store_entry,
                "--stamp",
                realized_stamp.as_output(),
            ],
            hidden = [
                stamp
                for dep in runtime_infos
                for stamp in dep.runtime_realized_stamps
            ],
        ),
        category = "pkgs_realize",
        identifier = ctx.attrs.package_name,
        local_only = True,
        allow_cache_upload = False,
    )

    return [
        DefaultInfo(default_output = realized_stamp, other_outputs = [tree]),
        PkgsPackageInfo(
            build_closure = build_closure,
            foreign_runtime_entries = foreign_runtime_entries,
            is_foreign = ctx.attrs.foreign,
            logical_store_path = logical_store_path,
            name = ctx.attrs.package_name,
            output = ctx.attrs.output,
            realized_stamp = realized_stamp,
            runtime_closure = runtime_closure,
            runtime_realized_stamps = runtime_realized_stamps,
            store_entry = store_entry,
            store_path_key = store_path_key,
            tree = tree,
            version = ctx.attrs.version,
        ),
    ]

pkgs_package = rule(
    impl = _pkgs_package_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "foreign": attrs.bool(default = False),
        "identity": attrs.string(),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "realization_root": attrs.string(default = read_root_config("pkgs", "realization_root", LOGICAL_STORE_ROOT)),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "src": attrs.dep(providers = [DefaultInfo]),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_realizer": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_realize",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_seed_free_impl(ctx):
    packages = [dep[PkgsPackageInfo] for dep in ctx.attrs.packages]
    forbidden = [dep[PkgsPackageInfo] for dep in ctx.attrs.forbidden]

    leaked_runtime_entries = []
    for package in packages:
        leaked_runtime_entries.extend(package.foreign_runtime_entries)
    if leaked_runtime_entries:
        fail("runtime closure still contains foreign entries: {}".format(leaked_runtime_entries))

    stamp = ctx.actions.declare_output("{}.seed_free".format(ctx.label.name))
    args = cmd_args([
        ctx.attrs._verifier[RunInfo],
        "--stamp",
        stamp.as_output(),
    ])
    for package in packages:
        args.add("--input", package.tree)
    for dep in forbidden:
        args.add("--forbidden", dep.logical_store_path)

    ctx.actions.run(
        args,
        category = "pkgs_verify_seed_free",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = stamp)]

pkgs_seed_free = rule(
    impl = _pkgs_seed_free_impl,
    attrs = {
        "forbidden": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "packages": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_no_refs",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_elf_interpreters_impl(ctx):
    packages = [dep[PkgsPackageInfo] for dep in ctx.attrs.packages]
    interpreter = ctx.attrs.interpreter[PkgsPackageInfo]
    stamp = ctx.actions.declare_output("{}.elf_interpreters".format(ctx.label.name))
    args = cmd_args([
        ctx.attrs._verifier[RunInfo],
        "--expected-interpreter",
        "{}/{}".format(interpreter.logical_store_path, ctx.attrs.interpreter_relpath),
        "--stamp",
        stamp.as_output(),
    ])
    for package in packages:
        args.add("--input", package.tree)

    ctx.actions.run(
        args,
        category = "pkgs_verify_elf_interpreters",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = stamp)]

pkgs_elf_interpreters = rule(
    impl = _pkgs_elf_interpreters_impl,
    attrs = {
        "interpreter": attrs.dep(providers = [PkgsPackageInfo]),
        "interpreter_relpath": attrs.string(),
        "packages": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_elf_interpreters",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_make_install_tree_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]
    _, store_entry = _store_path_parts(ctx, _deps_by_role(ctx))
    out = ctx.actions.declare_output(ctx.label.name, dir = True)

    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        out.as_output(),
        "--install-prefix",
        "{}/{}".format(LOGICAL_STORE_ROOT, store_entry),
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/{}/bin".format(ctx.attrs.realization_root, dep.store_entry),
        )
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))
    for arg in ctx.attrs.install_args:
        args.add("--install-arg={}".format(arg))
    for patch in ctx.attrs.patches:
        args.add("--patch", patch[DefaultInfo].default_outputs[0])
    args.add("--patch-strip", str(ctx.attrs.patch_strip))
    for link, target in ctx.attrs.symlinks.items():
        args.add("--symlink", "{}={}".format(link, target))

    native_runtime_stamps = [
        stamp
        for dep in native_build_inputs
        for stamp in dep.runtime_realized_stamps
    ]

    ctx.actions.run(
        cmd_args(
            args,
            hidden = native_runtime_stamps + [dep.realized_stamp for dep in build_inputs],
        ),
        category = "pkgs_make_install",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = out)]

_pkgs_make_install_tree = rule(
    impl = _pkgs_make_install_tree_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "identity": attrs.string(),
        "install_args": attrs.list(attrs.string(), default = []),
        "make_args": attrs.list(attrs.string(), default = []),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "patch_strip": attrs.int(default = 1),
        "patches": attrs.list(attrs.dep(providers = [DefaultInfo]), default = []),
        "realization_root": attrs.string(default = read_root_config("pkgs", "realization_root", LOGICAL_STORE_ROOT)),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source": attrs.dep(providers = [DefaultInfo]),
        "symlinks": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_make_install",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_linux_headers_tree_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    out = ctx.actions.declare_output(ctx.label.name, dir = True)

    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        out.as_output(),
        "--kernel-release",
        ctx.attrs.kernel_release,
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/{}/bin".format(ctx.attrs.realization_root, dep.store_entry),
        )
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))

    native_runtime_stamps = [
        stamp
        for dep in native_build_inputs
        for stamp in dep.runtime_realized_stamps
    ]

    ctx.actions.run(
        cmd_args(
            args,
            hidden = native_runtime_stamps,
        ),
        category = "pkgs_linux_headers_install",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = out)]

_pkgs_linux_headers_tree = rule(
    impl = _pkgs_linux_headers_tree_impl,
    attrs = {
        "kernel_release": attrs.string(),
        "make_args": attrs.list(attrs.string(), default = []),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "realization_root": attrs.string(default = read_root_config("pkgs", "realization_root", LOGICAL_STORE_ROOT)),
        "source": attrs.dep(providers = [DefaultInfo]),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_linux_headers_install",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_configure_make_install_tree_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]
    _, store_entry = _store_path_parts(ctx, _deps_by_role(ctx))
    out = ctx.actions.declare_output(ctx.label.name, dir = True)

    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        out.as_output(),
        "--install-prefix",
        "{}/{}".format(LOGICAL_STORE_ROOT, store_entry),
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/{}/bin".format(ctx.attrs.realization_root, dep.store_entry),
        )
    for arg in ctx.attrs.configure_args:
        args.add("--configure-arg={}".format(arg))
    if ctx.attrs.out_of_source:
        args.add("--out-of-source")
    for template, dep in ctx.attrs.configure_arg_inputs.items():
        args.add("--configure-arg={}".format(template.format(dep[PkgsPackageInfo].logical_store_path)))
    for env in ctx.attrs.configure_env:
        args.add("--configure-env={}".format(env))
    for template, dep in ctx.attrs.configure_env_inputs.items():
        args.add("--configure-env={}".format(template.format(dep[PkgsPackageInfo].logical_store_path)))
    for template, deps in ctx.attrs.configure_env_inputs_multi.items():
        args.add("--configure-env={}".format(template.format(*[dep[PkgsPackageInfo].logical_store_path for dep in deps])))
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))
    for arg in ctx.attrs.install_args:
        args.add("--install-arg={}".format(arg))
    for patch in ctx.attrs.patches:
        args.add("--patch", patch[DefaultInfo].default_outputs[0])
    args.add("--patch-strip", str(ctx.attrs.patch_strip))
    for link, target in ctx.attrs.symlinks.items():
        args.add("--symlink", "{}={}".format(link, target))

    native_runtime_stamps = [
        stamp
        for dep in native_build_inputs
        for stamp in dep.runtime_realized_stamps
    ]

    ctx.actions.run(
        cmd_args(
            args,
            hidden = native_runtime_stamps + [dep.realized_stamp for dep in build_inputs],
        ),
        category = "pkgs_configure_make_install",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = out)]

_pkgs_configure_make_install_tree = rule(
    impl = _pkgs_configure_make_install_tree_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "configure_arg_inputs": attrs.dict(attrs.string(), attrs.dep(providers = [PkgsPackageInfo]), default = {}),
        "configure_args": attrs.list(attrs.string(), default = []),
        "configure_env": attrs.list(attrs.string(), default = []),
        "configure_env_inputs": attrs.dict(attrs.string(), attrs.dep(providers = [PkgsPackageInfo]), default = {}),
        "configure_env_inputs_multi": attrs.dict(attrs.string(), attrs.list(attrs.dep(providers = [PkgsPackageInfo])), default = {}),
        "identity": attrs.string(),
        "install_args": attrs.list(attrs.string(), default = []),
        "make_args": attrs.list(attrs.string(), default = []),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "out_of_source": attrs.bool(default = False),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "patch_strip": attrs.int(default = 1),
        "patches": attrs.list(attrs.dep(providers = [DefaultInfo]), default = []),
        "realization_root": attrs.string(default = read_root_config("pkgs", "realization_root", LOGICAL_STORE_ROOT)),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source": attrs.dep(providers = [DefaultInfo]),
        "symlinks": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_configure_make_install",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_cc_wrapper_tree_impl(ctx):
    cc = ctx.attrs.cc[PkgsPackageInfo]
    bintools = ctx.attrs.bintools[PkgsPackageInfo]
    headers = ctx.attrs.headers[PkgsPackageInfo]
    libc = ctx.attrs.libc[PkgsPackageInfo]
    shell = ctx.attrs.shell[PkgsPackageInfo]
    out = ctx.actions.declare_output(ctx.label.name, dir = True)

    ctx.actions.run(
        cmd_args([
            ctx.attrs._builder[RunInfo],
            "--output",
            out.as_output(),
            "--shell",
            "{}/bin/bash".format(shell.logical_store_path),
            "--cc",
            "{}/bin/gcc".format(cc.logical_store_path),
            "--cxx",
            "{}/bin/g++".format(cc.logical_store_path),
            "--cpp",
            "{}/bin/cpp".format(cc.logical_store_path),
            "--libc",
            libc.logical_store_path,
            "--bintools",
            bintools.logical_store_path,
            "--headers",
            headers.logical_store_path,
            "--dynamic-linker",
            "{}/lib/ld-linux-x86-64.so.2".format(libc.logical_store_path),
        ]),
        category = "pkgs_cc_wrapper_tree",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = out)]

_pkgs_cc_wrapper_tree = rule(
    impl = _pkgs_cc_wrapper_tree_impl,
    attrs = {
        "bintools": attrs.dep(providers = [PkgsPackageInfo]),
        "cc": attrs.dep(providers = [PkgsPackageInfo]),
        "headers": attrs.dep(providers = [PkgsPackageInfo]),
        "libc": attrs.dep(providers = [PkgsPackageInfo]),
        "shell": attrs.dep(providers = [PkgsPackageInfo]),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_cc_wrapper_tree",
                providers = [RunInfo],
            ),
        ),
    },
)

def _pkgs_bintools_wrapper_tree_impl(ctx):
    bintools = ctx.attrs.bintools[PkgsPackageInfo]
    shell = ctx.attrs.shell[PkgsPackageInfo]
    out = ctx.actions.declare_output(ctx.label.name, dir = True)

    ctx.actions.run(
        cmd_args([
            ctx.attrs._builder[RunInfo],
            "--output",
            out.as_output(),
            "--shell",
            "{}/bin/bash".format(shell.logical_store_path),
            "--binutils",
            bintools.logical_store_path,
        ]),
        category = "pkgs_bintools_wrapper_tree",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = out)]

_pkgs_bintools_wrapper_tree = rule(
    impl = _pkgs_bintools_wrapper_tree_impl,
    attrs = {
        "bintools": attrs.dep(providers = [PkgsPackageInfo]),
        "shell": attrs.dep(providers = [PkgsPackageInfo]),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_bintools_wrapper_tree",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_make_install_package(
        name,
        package_name,
        version,
        source,
        identity,
        make_args = [],
        install_args = [],
        patches = [],
        patch_strip = 1,
        symlinks = {},
        output = "out",
        native_build_inputs = [],
        build_inputs = [],
        runtime_inputs = [],
        visibility = []):
    tree_name = name + "__tree"
    _pkgs_make_install_tree(
        name = tree_name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "make-install-v5",
        identity = identity,
        source = source,
        make_args = make_args,
        install_args = install_args,
        patches = patches,
        patch_strip = patch_strip,
        symlinks = symlinks,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        target_inputs = [],
        runtime_inputs = runtime_inputs,
    )
    pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "make-install-v5",
        identity = identity,
        src = ":" + tree_name,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def pkgs_configure_make_install_package(
        name,
        package_name,
        version,
        source,
        identity,
        configure_args = [],
        configure_arg_inputs = {},
        configure_env = [],
        configure_env_inputs = {},
        configure_env_inputs_multi = {},
        out_of_source = False,
        make_args = [],
        install_args = [],
        patches = [],
        patch_strip = 1,
        symlinks = {},
        output = "out",
        native_build_inputs = [],
        build_inputs = [],
        runtime_inputs = [],
        visibility = []):
    tree_name = name + "__tree"
    _pkgs_configure_make_install_tree(
        name = tree_name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "configure-make-install-v5",
        identity = identity,
        source = source,
        configure_args = configure_args,
        configure_arg_inputs = configure_arg_inputs,
        configure_env = configure_env,
        configure_env_inputs = configure_env_inputs,
        configure_env_inputs_multi = configure_env_inputs_multi,
        out_of_source = out_of_source,
        make_args = make_args,
        install_args = install_args,
        patches = patches,
        patch_strip = patch_strip,
        symlinks = symlinks,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        target_inputs = [],
        runtime_inputs = runtime_inputs,
    )
    pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "configure-make-install-v5",
        identity = identity,
        src = ":" + tree_name,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def pkgs_linux_headers_package(
        name,
        package_name,
        version,
        source,
        identity,
        kernel_release,
        make_args = [],
        output = "out",
        native_build_inputs = [],
        visibility = []):
    tree_name = name + "__tree"
    _pkgs_linux_headers_tree(
        name = tree_name,
        source = source,
        kernel_release = kernel_release,
        make_args = make_args,
        native_build_inputs = native_build_inputs,
    )
    pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "linux-headers-install-v1",
        identity = identity,
        src = ":" + tree_name,
        native_build_inputs = native_build_inputs,
        visibility = visibility,
    )

def pkgs_cc_wrapper_package(
        name,
        package_name,
        version,
        identity,
        cc,
        bintools,
        headers,
        libc,
        shell,
        output = "bin",
        visibility = []):
    tree_name = name + "__tree"
    _pkgs_cc_wrapper_tree(
        name = tree_name,
        cc = cc,
        bintools = bintools,
        headers = headers,
        libc = libc,
        shell = shell,
    )
    pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "cc-wrapper-tree-v0",
        identity = identity,
        src = ":" + tree_name,
        runtime_inputs = [
            cc,
            bintools,
            headers,
            libc,
            shell,
        ],
        visibility = visibility,
    )

def pkgs_bintools_wrapper_package(
        name,
        package_name,
        version,
        identity,
        bintools,
        shell,
        output = "bin",
        visibility = []):
    tree_name = name + "__tree"
    _pkgs_bintools_wrapper_tree(
        name = tree_name,
        bintools = bintools,
        shell = shell,
    )
    pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "bintools-wrapper-tree-v0",
        identity = identity,
        src = ":" + tree_name,
        runtime_inputs = [
            bintools,
            shell,
        ],
        visibility = visibility,
    )
