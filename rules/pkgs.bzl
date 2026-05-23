load("@prelude//utils:selects.bzl", "selects")

STORE_ABI_VERSION = "pkgs-store-v0"
LOGICAL_STORE_ROOT = "/pkgs/store"
DEFAULT_MAKE_JOBS = 64

_TARGET_SYSTEM = select({
    "//platforms:linux_arm64": "aarch64-linux",
    "//platforms:linux_x86_64": "x86_64-linux",
})

_DYNAMIC_LINKER = select({
    "//platforms:linux_arm64": "lib/ld-linux-aarch64.so.1",
    "//platforms:linux_x86_64": "lib/ld-linux-x86-64.so.2",
})

PkgsPackageInfo = provider(
    fields = [
        "build_closure",
        "foreign_runtime_entries",
        "is_foreign",
        "logical_store_path",
        "name",
        "output",
        "runtime_closure",
        "runtime_store_outputs",
        "store_entry",
        "store_output",
        "store_path_key",
        "tree",
        "version",
    ],
)

PkgsStoreSubstituteInfo = provider(
    fields = [
        "archive",
        "logical_store_path",
        "manifest",
        "name",
        "output",
        "references",
        "runtime_store_entries",
        "store_entry",
        "store_name",
        "store_path_key",
        "target_system",
        "version",
    ],
)

def pkgs_source(dep, digests = []):
    return struct(
        dep = dep,
        digests = digests,
    )

def pkgs_patch(dep, digests = []):
    return struct(
        dep = dep,
        digests = digests,
    )

def pkgs_http_archive_source(name, sha256, strip_prefix, urls, visibility = []):
    native.http_archive(
        name = name,
        sha256 = sha256,
        strip_prefix = strip_prefix,
        urls = urls,
        visibility = visibility,
    )
    return pkgs_source(
        dep = ":" + name,
        digests = ["sha256:" + sha256],
    )

def pkgs_export_patch(name, src, sha256, visibility = []):
    native.export_file(
        name = name,
        src = src,
        visibility = visibility,
    )
    return pkgs_patch(
        dep = ":" + name,
        digests = ["sha256:" + sha256],
    )

def pkgs_store_path(prefix, dep, suffix = ""):
    return struct(
        _pkgs_configure_value_kind = "store_path",
        dep = dep,
        prefix = prefix,
        suffix = suffix,
    )

def pkgs_self_store_path(prefix, suffix = ""):
    return struct(
        _pkgs_configure_value_kind = "self_store_path",
        prefix = prefix,
        suffix = suffix,
    )

def pkgs_store_path_join(prefix, deps, separator, suffix = ""):
    return struct(
        _pkgs_configure_value_kind = "store_path_join",
        deps = deps,
        prefix = prefix,
        separator = separator,
        suffix = suffix,
    )

def _descriptor_deps(descriptors):
    return [descriptor.dep for descriptor in descriptors]

def _descriptor_digests(descriptors):
    digests = []
    for descriptor in descriptors:
        digests.extend(descriptor.digests)
    return digests

def _select_aware(value, transform):
    return selects.apply(value, transform) if selects.is_select(value) else transform(value)

def _composed_source_digests(sources):
    return [
        "source-layout=compose-sources-v0",
        "source-count={}".format(len(sources)),
    ] + _descriptor_digests(sources)

def _compose_source_set(name, sources):
    source_set_name = "{}__sources".format(name)
    _pkgs_compose_sources(
        name = source_set_name,
        sources = _select_aware(sources, _descriptor_deps),
    )
    return pkgs_source(
        dep = ":" + source_set_name,
        digests = _select_aware(sources, _composed_source_digests),
    )

def _source_set(name, sources):
    if selects.is_select(sources):
        return _compose_source_set(name, sources)

    if len(sources) == 0:
        fail("package {} must declare at least one source".format(name))

    if len(sources) == 1:
        return sources[0]

    return _compose_source_set(name, sources)

def _lower_configure_values(values):
    strings = []
    store_paths = []
    self_store_paths = []
    store_path_joins = []

    for value in values:
        kind = getattr(value, "_pkgs_configure_value_kind", None)
        if kind == None:
            strings.append(value)
        elif kind == "store_path":
            store_paths.append((value.prefix, value.dep, value.suffix))
        elif kind == "self_store_path":
            self_store_paths.append((value.prefix, value.suffix))
        elif kind == "store_path_join":
            store_path_joins.append((value.prefix, value.deps, value.separator, value.suffix))
        else:
            fail("unsupported package configure value kind: {}".format(kind))

    return struct(
        self_store_paths = self_store_paths,
        store_path_joins = store_path_joins,
        store_paths = store_paths,
        strings = strings,
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
        "output={}".format(ctx.attrs.output),
        "target_system={}".format(ctx.attrs.target_system),
    ]
    parts.extend(_recipe_semantic_parts(ctx))
    _append_semantic_values(parts, "source_digest", getattr(ctx.attrs, "source_digests", []))
    _append_semantic_values(parts, "patch_digest", getattr(ctx.attrs, "patch_digests", []))

    for role, deps in deps_by_role:
        parts.append("role={}".format(role))
        for dep in deps:
            parts.append(dep.logical_store_path)

    return sha256("\n".join(parts))

def _append_semantic_values(parts, key, values):
    for value in values:
        parts.append("{}={}".format(key, value))

def _recipe_semantic_parts(ctx):
    parts = []

    _append_semantic_values(parts, "configure_arg", getattr(ctx.attrs, "configure_args", []))
    for prefix, dep, suffix in getattr(ctx.attrs, "configure_arg_store_paths", []):
        parts.append("configure_arg_store_path={}{}{}".format(
            prefix,
            dep[PkgsPackageInfo].logical_store_path,
            suffix,
        ))
    for prefix, suffix in getattr(ctx.attrs, "configure_arg_self_store_paths", []):
        parts.append("configure_arg_self_store_path={}<self>{}".format(prefix, suffix))
    for prefix, deps, separator, suffix in getattr(ctx.attrs, "configure_arg_store_path_joins", []):
        parts.append("configure_arg_store_path_join={}{}{}".format(
            prefix,
            separator.join([dep[PkgsPackageInfo].logical_store_path for dep in deps]),
            suffix,
        ))

    _append_semantic_values(parts, "configure_env", getattr(ctx.attrs, "configure_env", []))
    for prefix, dep, suffix in getattr(ctx.attrs, "configure_env_store_paths", []):
        parts.append("configure_env_store_path={}{}{}".format(
            prefix,
            dep[PkgsPackageInfo].logical_store_path,
            suffix,
        ))
    for prefix, suffix in getattr(ctx.attrs, "configure_env_self_store_paths", []):
        parts.append("configure_env_self_store_path={}<self>{}".format(prefix, suffix))
    for prefix, deps, separator, suffix in getattr(ctx.attrs, "configure_env_store_path_joins", []):
        parts.append("configure_env_store_path_join={}{}{}".format(
            prefix,
            separator.join([dep[PkgsPackageInfo].logical_store_path for dep in deps]),
            suffix,
        ))

    if getattr(ctx.attrs, "out_of_source", False):
        parts.append("out_of_source=true")

    _append_semantic_values(parts, "make_arg", getattr(ctx.attrs, "make_args", []))
    if hasattr(ctx.attrs, "make_jobs"):
        parts.append("make_jobs={}".format(ctx.attrs.make_jobs))
    _append_semantic_values(parts, "install_arg", getattr(ctx.attrs, "install_args", []))

    if hasattr(ctx.attrs, "patch_strip"):
        parts.append("patch_strip={}".format(ctx.attrs.patch_strip))

    for link, target in sorted(getattr(ctx.attrs, "symlinks", {}).items()):
        parts.append("symlink={}={}".format(link, target))

    if hasattr(ctx.attrs, "kernel_release"):
        parts.append("kernel_release={}".format(ctx.attrs.kernel_release))

    if hasattr(ctx.attrs, "dynamic_linker"):
        parts.append("dynamic_linker={}".format(ctx.attrs.dynamic_linker))

    return parts

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
    return (store_path_key, store_name, store_entry)

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

def _collect_runtime_store_outputs(store_entry, store_output, deps):
    seen = {}
    entries = []
    outputs = []

    for dep in deps:
        for entry, output in zip(dep.runtime_closure, dep.runtime_store_outputs):
            if entry not in seen:
                seen[entry] = True
                entries.append(entry)
                outputs.append(output)

    if store_entry not in seen:
        entries.append(store_entry)
        outputs.append(store_output)

    return outputs

def _logical_store_paths(entries):
    return ["{}/{}".format(LOGICAL_STORE_ROOT, entry) for entry in entries]

def _runtime_manifest_entries(store_entry, deps):
    runtime_closure = _collect_closure(
        store_entry,
        [dep.runtime_closure for dep in deps],
    )
    references = [entry for entry in runtime_closure if entry != store_entry]
    return (runtime_closure, references)

def _canonical_runtime_manifest_entries(store_entry, deps):
    runtime_closure, _ = _runtime_manifest_entries(store_entry, deps)
    runtime_closure = sorted(runtime_closure)
    references = [entry for entry in runtime_closure if entry != store_entry]
    return (runtime_closure, references)

def _runtime_store_outputs_for_entries(entries, store_entry, store_output, deps):
    outputs_by_entry = {}

    for dep in deps:
        for entry, output in zip(dep.runtime_closure, dep.runtime_store_outputs):
            if entry not in outputs_by_entry:
                outputs_by_entry[entry] = output

    outputs_by_entry[store_entry] = store_output
    return [outputs_by_entry[entry] for entry in entries]

def _deps_by_role(ctx):
    return [
        ("native_build_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]),
        ("build_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]),
        ("target_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.target_inputs]),
        ("runtime_inputs", [dep[PkgsPackageInfo] for dep in ctx.attrs.runtime_inputs]),
    ]

def _package_metadata(ctx):
    deps_by_role = _deps_by_role(ctx)
    store_path_key, store_name, store_entry = _store_path_parts(ctx, deps_by_role)
    logical_store_path = "{}/{}".format(LOGICAL_STORE_ROOT, store_entry)
    store_path = ctx.actions.store_path(
        store_path_key = store_path_key,
        store_name = store_name,
    )
    store_output = ctx.actions.declare_store_output(
        store_path = store_path,
        dir = True,
    )
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
    runtime_store_outputs = _collect_runtime_store_outputs(
        store_entry,
        store_output,
        runtime_infos,
    )
    foreign_runtime_entries = _collect_entries(
        [dep.foreign_runtime_entries for dep in runtime_infos],
    )
    if ctx.attrs.foreign:
        foreign_runtime_entries = _collect_closure(store_entry, [foreign_runtime_entries])

    return struct(
        build_closure = build_closure,
        foreign_runtime_entries = foreign_runtime_entries,
        logical_store_path = logical_store_path,
        runtime_closure = runtime_closure,
        runtime_store_outputs = runtime_store_outputs,
        store_entry = store_entry,
        store_output = store_output,
        store_path_key = store_path_key,
    )

def _declare_reproducibility_stamp(ctx, metadata, replay_tree):
    stamp = ctx.actions.declare_output("{}.reproducible".format(ctx.label.name))
    ctx.actions.run(
        cmd_args([
            ctx.attrs._reproducibility_verifier[RunInfo],
            "--expected",
            metadata.store_output,
            "--actual",
            replay_tree,
            "--stamp",
            stamp.as_output(),
        ]),
        category = "pkgs_verify_reproducible_tree",
        identifier = ctx.label.name,
    )
    return stamp

def _declare_archive_metadata_stamp(ctx, metadata):
    stamp = ctx.actions.declare_output("{}.archive_metadata".format(ctx.label.name))
    ctx.actions.run(
        cmd_args([
            ctx.attrs._archive_metadata_verifier[RunInfo],
            "--input",
            metadata.store_output,
            "--stamp",
            stamp.as_output(),
        ]),
        category = "pkgs_verify_archive_metadata",
        identifier = ctx.label.name,
    )
    return stamp

def _package_sub_targets(reproducibility_stamp, archive_metadata_stamp):
    return {
        "archive_metadata": [DefaultInfo(default_output = archive_metadata_stamp)],
        "reproducible": [DefaultInfo(default_output = reproducibility_stamp)],
    }

def _package_result(ctx, tree, metadata, reproducibility_stamp, archive_metadata_stamp, other_outputs = []):
    return [
        DefaultInfo(
            default_output = metadata.store_output,
            other_outputs = other_outputs,
            sub_targets = _package_sub_targets(reproducibility_stamp, archive_metadata_stamp),
        ),
        PkgsPackageInfo(
            build_closure = metadata.build_closure,
            foreign_runtime_entries = metadata.foreign_runtime_entries,
            is_foreign = ctx.attrs.foreign,
            logical_store_path = metadata.logical_store_path,
            name = ctx.attrs.package_name,
            output = ctx.attrs.output,
            runtime_closure = metadata.runtime_closure,
            runtime_store_outputs = metadata.runtime_store_outputs,
            store_entry = metadata.store_entry,
            store_output = metadata.store_output,
            store_path_key = metadata.store_path_key,
            tree = tree,
            version = ctx.attrs.version,
        ),
    ]

def _stage_tree_package(ctx, tree):
    metadata = _package_metadata(ctx)

    ctx.actions.run(
        cmd_args(
            [
                ctx.attrs._tree_stager[RunInfo],
                "--source",
                tree,
                "--output",
                metadata.store_output.as_output(),
            ],
            hidden = [
                output
                for dep in ctx.attrs.runtime_inputs
                for output in dep[PkgsPackageInfo].runtime_store_outputs
            ],
        ),
        category = "pkgs_stage_tree",
        identifier = ctx.attrs.package_name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        cmd_args([
            ctx.attrs._tree_stager[RunInfo],
            "--source",
            tree,
            "--output",
            replay_tree.as_output(),
        ]),
        category = "pkgs_replay_stage_tree",
        identifier = ctx.attrs.package_name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
        other_outputs = [tree],
    )

def _pkgs_package_impl(ctx):
    return _stage_tree_package(ctx, ctx.attrs.src[DefaultInfo].default_outputs[0])

def _pkgs_compose_sources_impl(ctx):
    output = ctx.actions.declare_output("{}.sources".format(ctx.label.name), dir = True)
    args = cmd_args([
        ctx.attrs._composer[RunInfo],
        "--output",
        output.as_output(),
    ])
    for source in ctx.attrs.sources:
        args.add("--source", source[DefaultInfo].default_outputs[0])

    ctx.actions.run(
        args,
        category = "pkgs_compose_sources",
        identifier = ctx.label.name,
    )

    return [DefaultInfo(default_output = output)]

_pkgs_compose_sources = rule(
    impl = _pkgs_compose_sources_impl,
    attrs = {
        "sources": attrs.list(attrs.dep(providers = [DefaultInfo])),
        "_composer": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_compose_sources",
                providers = [RunInfo],
            ),
        ),
    },
)

_pkgs_package = rule(
    impl = _pkgs_package_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "foreign": attrs.bool(default = False),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source_digests": attrs.list(attrs.string(), default = []),
        "src": attrs.dep(providers = [DefaultInfo]),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_tree_stager": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_stage_tree",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_package(
        name,
        package_name,
        version,
        builder,
        sources,
        output = "out",
        foreign = False,
        native_build_inputs = [],
        build_inputs = [],
        target_inputs = [],
        runtime_inputs = [],
        visibility = []):
    source = _source_set(name, sources)
    _pkgs_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = builder,
        foreign = foreign,
        source_digests = source.digests,
        src = source.dep,
        target_system = _TARGET_SYSTEM,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        target_inputs = target_inputs,
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def _declare_store_substitute(
        ctx,
        tree,
        store_path_key,
        store_name,
        package_name,
        version,
        output,
        target_system,
        runtime_store_entries):
    runtime_store_entries = sorted(_collect_entries([runtime_store_entries]))
    store_entry = "{}-{}".format(store_path_key, store_name)
    logical_store_path = "{}/{}".format(LOGICAL_STORE_ROOT, store_entry)
    archive = ctx.actions.declare_output("{}.bpkgs-tree".format(ctx.label.name))
    manifest = ctx.actions.declare_output("{}.manifest.json".format(ctx.label.name))
    references = [
        entry
        for entry in runtime_store_entries
        if entry != store_entry
    ]
    args = cmd_args([
        ctx.attrs._exporter[RunInfo],
        "--input",
        tree,
        "--store-path",
        logical_store_path,
        "--store-path-key",
        store_path_key,
        "--store-entry",
        store_entry,
        "--package-name",
        package_name,
        "--version",
        version,
        "--output",
        output,
        "--target-system",
        target_system,
        "--archive",
        archive.as_output(),
        "--manifest",
        manifest.as_output(),
    ])
    for entry in references:
        args.add("--reference", "{}/{}".format(LOGICAL_STORE_ROOT, entry))
    for entry in runtime_store_entries:
        args.add("--runtime-store-output", "{}/{}".format(LOGICAL_STORE_ROOT, entry))

    ctx.actions.run(
        args,
        category = "pkgs_export_store_substitute",
        identifier = ctx.label.name,
    )

    return [
        DefaultInfo(default_output = manifest, other_outputs = [archive]),
        PkgsStoreSubstituteInfo(
            archive = archive,
            logical_store_path = logical_store_path,
            manifest = manifest,
            name = package_name,
            output = output,
            references = _logical_store_paths(references),
            runtime_store_entries = runtime_store_entries,
            store_entry = store_entry,
            store_name = store_name,
            store_path_key = store_path_key,
            target_system = target_system,
            version = version,
        ),
    ]

def _pkgs_export_store_substitute_impl(ctx):
    package = ctx.attrs.package[PkgsPackageInfo]
    return _declare_store_substitute(
        ctx,
        package.store_output,
        package.store_path_key,
        package.store_entry[len(package.store_path_key) + 1:],
        package.name,
        package.version,
        package.output,
        ctx.attrs.target_system,
        package.runtime_closure,
    )

_pkgs_export_store_substitute = rule(
    impl = _pkgs_export_store_substitute_impl,
    attrs = {
        "package": attrs.dep(providers = [PkgsPackageInfo]),
        "target_system": attrs.string(),
        "_exporter": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_export_store_object",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_export_store_substitute(name, package, visibility = []):
    _pkgs_export_store_substitute(
        name = name,
        package = package,
        target_system = _TARGET_SYSTEM,
        visibility = visibility,
    )

def _pkgs_export_store_closure_impl(ctx):
    output = ctx.actions.declare_output("{}.bundle".format(ctx.label.name), dir = True)
    args = cmd_args([
        ctx.attrs._exporter[RunInfo],
        "--name",
        ctx.attrs.closure_name,
        "--target-system",
        ctx.attrs.target_system,
        "--output",
        output.as_output(),
    ])
    for root in ctx.attrs.roots:
        args.add("--root", root[PkgsStoreSubstituteInfo].logical_store_path)
    for object in ctx.attrs.objects:
        substitute = object[PkgsStoreSubstituteInfo]
        args.add("--object-manifest", substitute.manifest)
        args.add("--object-archive", substitute.archive)
    ctx.actions.run(
        args,
        category = "pkgs_export_store_closure",
        identifier = ctx.label.name,
    )
    return [DefaultInfo(default_output = output)]

_pkgs_export_store_closure = rule(
    impl = _pkgs_export_store_closure_impl,
    attrs = {
        "closure_name": attrs.string(),
        "objects": attrs.list(attrs.dep(providers = [PkgsStoreSubstituteInfo])),
        "roots": attrs.list(attrs.dep(providers = [PkgsStoreSubstituteInfo])),
        "target_system": attrs.string(),
        "_exporter": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_export_store_closure",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_export_store_closure(name, closure_name, roots, objects, visibility = []):
    _pkgs_export_store_closure(
        name = name,
        closure_name = closure_name,
        roots = roots,
        objects = objects,
        target_system = _TARGET_SYSTEM,
        visibility = visibility,
    )

def _pkgs_export_store_tree_substitute_impl(ctx):
    store_entry = "{}-{}".format(ctx.attrs.store_path_key, ctx.attrs.store_name)
    runtime_store_entries = _collect_closure(store_entry, [ctx.attrs.runtime_store_entries])
    return _declare_store_substitute(
        ctx,
        ctx.attrs.tree[DefaultInfo].default_outputs[0],
        ctx.attrs.store_path_key,
        ctx.attrs.store_name,
        ctx.attrs.package_name,
        ctx.attrs.version,
        ctx.attrs.output,
        ctx.attrs.target_system,
        runtime_store_entries,
    )

_pkgs_export_store_tree_substitute = rule(
    impl = _pkgs_export_store_tree_substitute_impl,
    attrs = {
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "runtime_store_entries": attrs.list(attrs.string(), default = []),
        "store_name": attrs.string(),
        "store_path_key": attrs.string(),
        "target_system": attrs.string(),
        "tree": attrs.dep(providers = [DefaultInfo]),
        "version": attrs.string(),
        "_exporter": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_export_store_object",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_export_store_tree_substitute(
        name,
        tree,
        package_name,
        version,
        store_path_key,
        store_name,
        output = "out",
        runtime_store_entries = [],
        visibility = []):
    _pkgs_export_store_tree_substitute(
        name = name,
        tree = tree,
        package_name = package_name,
        version = version,
        output = output,
        store_path_key = store_path_key,
        store_name = store_name,
        target_system = _TARGET_SYSTEM,
        runtime_store_entries = runtime_store_entries,
        visibility = visibility,
    )

def _pkgs_prebuilt_store_substitute_impl(ctx):
    store_entry = "{}-{}".format(ctx.attrs.store_path_key, ctx.attrs.store_name)
    return [
        DefaultInfo(
            default_output = ctx.attrs.manifest[DefaultInfo].default_outputs[0],
            other_outputs = [ctx.attrs.archive[DefaultInfo].default_outputs[0]],
        ),
        PkgsStoreSubstituteInfo(
            archive = ctx.attrs.archive[DefaultInfo].default_outputs[0],
            logical_store_path = "{}/{}".format(LOGICAL_STORE_ROOT, store_entry),
            manifest = ctx.attrs.manifest[DefaultInfo].default_outputs[0],
            name = ctx.attrs.package_name,
            output = ctx.attrs.output,
            references = ctx.attrs.references,
            runtime_store_entries = ctx.attrs.runtime_store_entries,
            store_entry = store_entry,
            store_name = ctx.attrs.store_name,
            store_path_key = ctx.attrs.store_path_key,
            target_system = ctx.attrs.target_system,
            version = ctx.attrs.version,
        ),
    ]

_pkgs_prebuilt_store_substitute = rule(
    impl = _pkgs_prebuilt_store_substitute_impl,
    attrs = {
        "archive": attrs.dep(providers = [DefaultInfo]),
        "manifest": attrs.dep(providers = [DefaultInfo]),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "references": attrs.list(attrs.string(), default = []),
        "runtime_store_entries": attrs.list(attrs.string(), default = []),
        "store_name": attrs.string(),
        "store_path_key": attrs.string(),
        "target_system": attrs.string(),
        "version": attrs.string(),
    },
)

def pkgs_prebuilt_store_substitute(
        name,
        archive,
        manifest,
        package_name,
        version,
        store_path_key,
        store_name,
        target_system,
        output = "out",
        references = [],
        runtime_store_entries = [],
        visibility = []):
    _pkgs_prebuilt_store_substitute(
        name = name,
        archive = archive,
        manifest = manifest,
        package_name = package_name,
        version = version,
        output = output,
        store_path_key = store_path_key,
        store_name = store_name,
        target_system = target_system,
        references = references,
        runtime_store_entries = runtime_store_entries,
        visibility = visibility,
    )

def _pkgs_imported_store_output_impl(ctx):
    substitute = ctx.attrs.substitute[PkgsStoreSubstituteInfo]
    runtime_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.runtime_inputs]
    runtime_closure, references = _canonical_runtime_manifest_entries(substitute.store_entry, runtime_inputs)
    if runtime_closure != substitute.runtime_store_entries:
        fail("imported package runtime closure does not match substitute manifest metadata for {}".format(substitute.logical_store_path))
    if _logical_store_paths(references) != substitute.references:
        fail("imported package references do not match substitute manifest metadata for {}".format(substitute.logical_store_path))

    store_path = ctx.actions.store_path(
        store_path_key = substitute.store_path_key,
        store_name = substitute.store_name,
    )
    store_output = ctx.actions.declare_store_output(
        store_path = store_path,
        dir = True,
    )
    runtime_store_outputs = _runtime_store_outputs_for_entries(
        runtime_closure,
        substitute.store_entry,
        store_output,
        runtime_inputs,
    )
    args = cmd_args([
        ctx.attrs._importer[RunInfo],
        "--manifest",
        substitute.manifest,
        "--archive",
        substitute.archive,
        "--expected-store-path",
        substitute.logical_store_path,
        "--expected-package-name",
        substitute.name,
        "--expected-version",
        substitute.version,
        "--expected-output",
        substitute.output,
        "--expected-target-system",
        substitute.target_system,
        "--output",
        store_output.as_output(),
    ])
    for reference in substitute.references:
        args.add("--expected-reference", reference)
    for entry in substitute.runtime_store_entries:
        args.add("--expected-runtime-store-output", "{}/{}".format(LOGICAL_STORE_ROOT, entry))
    ctx.actions.run(
        args,
        category = "pkgs_import_store_object",
        identifier = ctx.label.name,
    )

    return [
        DefaultInfo(default_output = store_output),
        PkgsPackageInfo(
            build_closure = runtime_closure,
            foreign_runtime_entries = [],
            is_foreign = False,
            logical_store_path = substitute.logical_store_path,
            name = substitute.name,
            output = substitute.output,
            runtime_closure = runtime_closure,
            runtime_store_outputs = runtime_store_outputs,
            store_entry = substitute.store_entry,
            store_output = store_output,
            store_path_key = substitute.store_path_key,
            tree = store_output,
            version = substitute.version,
        ),
    ]

_pkgs_imported_store_output = rule(
    impl = _pkgs_imported_store_output_impl,
    attrs = {
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "substitute": attrs.dep(providers = [PkgsStoreSubstituteInfo]),
        "_importer": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_import_store_object",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_imported_store_output(name, substitute, runtime_inputs = [], visibility = []):
    _pkgs_imported_store_output(
        name = name,
        substitute = substitute,
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def _pkgs_hydrated_store_output_impl(ctx):
    store_entry = "{}-{}".format(ctx.attrs.store_path_key, ctx.attrs.store_name)
    logical_store_path = "{}/{}".format(LOGICAL_STORE_ROOT, store_entry)
    runtime_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.runtime_inputs]
    runtime_closure, references = _canonical_runtime_manifest_entries(store_entry, runtime_inputs)
    if runtime_closure != ctx.attrs.runtime_store_entries:
        fail("hydrated package runtime closure does not match pinned manifest metadata for {}".format(logical_store_path))
    if _logical_store_paths(references) != ctx.attrs.references:
        fail("hydrated package references do not match pinned manifest metadata for {}".format(logical_store_path))

    store_path = ctx.actions.store_path(
        store_path_key = ctx.attrs.store_path_key,
        store_name = ctx.attrs.store_name,
    )
    store_output = ctx.actions.declare_store_output(
        store_path = store_path,
        dir = True,
    )
    runtime_store_outputs = _runtime_store_outputs_for_entries(
        runtime_closure,
        store_entry,
        store_output,
        runtime_inputs,
    )
    args = cmd_args([
        ctx.attrs._projector[RunInfo],
        "--manifest",
        ctx.attrs.manifest[DefaultInfo].default_outputs[0],
        "--expected-store-path",
        logical_store_path,
        "--expected-package-name",
        ctx.attrs.package_name,
        "--expected-version",
        ctx.attrs.version,
        "--expected-output",
        ctx.attrs.output,
        "--expected-target-system",
        ctx.attrs.target_system,
        "--missing-hint",
        ctx.attrs.missing_hint,
        "--output",
        store_output.as_output(),
    ])
    for reference in ctx.attrs.references:
        args.add("--expected-reference", reference)
    for entry in ctx.attrs.runtime_store_entries:
        args.add("--expected-runtime-store-output", "{}/{}".format(LOGICAL_STORE_ROOT, entry))
    ctx.actions.run(
        args,
        category = "pkgs_project_hydrated_store_object",
        identifier = ctx.label.name,
        local_only = True,
        allow_cache_upload = False,
    )
    return [
        DefaultInfo(default_output = store_output),
        PkgsPackageInfo(
            build_closure = runtime_closure,
            foreign_runtime_entries = [],
            is_foreign = False,
            logical_store_path = logical_store_path,
            name = ctx.attrs.package_name,
            output = ctx.attrs.output,
            runtime_closure = runtime_closure,
            runtime_store_outputs = runtime_store_outputs,
            store_entry = store_entry,
            store_output = store_output,
            store_path_key = ctx.attrs.store_path_key,
            tree = store_output,
            version = ctx.attrs.version,
        ),
    ]

_pkgs_hydrated_store_output = rule(
    impl = _pkgs_hydrated_store_output_impl,
    attrs = {
        "manifest": attrs.dep(providers = [DefaultInfo]),
        "missing_hint": attrs.string(),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "references": attrs.list(attrs.string(), default = []),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "runtime_store_entries": attrs.list(attrs.string(), default = []),
        "store_name": attrs.string(),
        "store_path_key": attrs.string(),
        "target_system": attrs.string(),
        "version": attrs.string(),
        "_projector": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_project_hydrated_store_object",
                providers = [RunInfo],
            ),
        ),
    },
)

def pkgs_hydrated_store_output(
        name,
        manifest,
        package_name,
        version,
        store_path_key,
        store_name,
        target_system,
        missing_hint,
        output = "out",
        references = [],
        runtime_store_entries = [],
        runtime_inputs = [],
        visibility = []):
    _pkgs_hydrated_store_output(
        name = name,
        manifest = manifest,
        package_name = package_name,
        version = version,
        output = output,
        store_path_key = store_path_key,
        store_name = store_name,
        target_system = target_system,
        missing_hint = missing_hint,
        references = references,
        runtime_store_entries = runtime_store_entries,
        runtime_inputs = runtime_inputs,
        visibility = visibility,
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

def _make_install_args(ctx, source, output, metadata, native_build_inputs):
    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        output,
        "--install-prefix",
        metadata.logical_store_path,
        "--make-jobs",
        str(ctx.attrs.make_jobs),
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/bin".format(dep.logical_store_path),
        )
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))
    for arg in ctx.attrs.install_args:
        args.add("--install-arg={}".format(arg))
    for patch in ctx.attrs.patches:
        args.add("--patch", patch[DefaultInfo].default_outputs[0])
    args.add("--patch-strip", str(ctx.attrs.patch_strip))
    for link, target in sorted(ctx.attrs.symlinks.items()):
        args.add("--symlink", "{}={}".format(link, target))
    return args

def _pkgs_make_install_package_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]
    metadata = _package_metadata(ctx)

    native_runtime_store_outputs = [
        output
        for dep in native_build_inputs
        for output in dep.runtime_store_outputs
    ]

    ctx.actions.run(
        cmd_args(
            _make_install_args(
                ctx,
                source,
                metadata.store_output.as_output(),
                metadata,
                native_build_inputs,
            ),
            hidden = native_runtime_store_outputs + [dep.store_output for dep in build_inputs],
        ),
        category = "pkgs_make_install",
        identifier = ctx.label.name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        cmd_args(
            _make_install_args(
                ctx,
                source,
                replay_tree.as_output(),
                metadata,
                native_build_inputs,
            ),
            hidden = native_runtime_store_outputs + [dep.store_output for dep in build_inputs],
        ),
        category = "pkgs_replay_make_install",
        identifier = ctx.label.name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
    )

_pkgs_make_install_package = rule(
    impl = _pkgs_make_install_package_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "foreign": attrs.bool(default = False),
        "install_args": attrs.list(attrs.string(), default = []),
        "make_args": attrs.list(attrs.string(), default = []),
        "make_jobs": attrs.int(default = DEFAULT_MAKE_JOBS),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "patch_digests": attrs.list(attrs.string(), default = []),
        "patch_strip": attrs.int(default = 1),
        "patches": attrs.list(attrs.dep(providers = [DefaultInfo]), default = []),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source": attrs.dep(providers = [DefaultInfo]),
        "source_digests": attrs.list(attrs.string(), default = []),
        "symlinks": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_make_install",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def _linux_headers_args(ctx, source, output, native_build_inputs):
    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        output,
        "--kernel-release",
        ctx.attrs.kernel_release,
        "--make-jobs",
        str(ctx.attrs.make_jobs),
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/bin".format(dep.logical_store_path),
        )
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))
    return args

def _pkgs_linux_headers_package_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    metadata = _package_metadata(ctx)

    native_runtime_store_outputs = [
        output
        for dep in native_build_inputs
        for output in dep.runtime_store_outputs
    ]

    ctx.actions.run(
        cmd_args(
            _linux_headers_args(
                ctx,
                source,
                metadata.store_output.as_output(),
                native_build_inputs,
            ),
            hidden = native_runtime_store_outputs,
        ),
        category = "pkgs_linux_headers_install",
        identifier = ctx.label.name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        cmd_args(
            _linux_headers_args(ctx, source, replay_tree.as_output(), native_build_inputs),
            hidden = native_runtime_store_outputs,
        ),
        category = "pkgs_replay_linux_headers_install",
        identifier = ctx.label.name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
    )

_pkgs_linux_headers_package = rule(
    impl = _pkgs_linux_headers_package_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "foreign": attrs.bool(default = False),
        "kernel_release": attrs.string(),
        "make_args": attrs.list(attrs.string(), default = []),
        "make_jobs": attrs.int(default = DEFAULT_MAKE_JOBS),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source": attrs.dep(providers = [DefaultInfo]),
        "source_digests": attrs.list(attrs.string(), default = []),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_linux_headers_install",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def _configure_make_install_args(ctx, source, output, metadata, native_build_inputs):
    self_store_path = metadata.logical_store_path

    args = cmd_args([
        ctx.attrs._builder[RunInfo],
        "--source",
        source,
        "--output",
        output,
        "--install-prefix",
        metadata.logical_store_path,
        "--make-jobs",
        str(ctx.attrs.make_jobs),
    ])
    for dep in native_build_inputs:
        args.add(
            "--path-entry",
            "{}/bin".format(dep.logical_store_path),
        )
    for arg in ctx.attrs.configure_args:
        args.add("--configure-arg={}".format(arg))
    if ctx.attrs.out_of_source:
        args.add("--out-of-source")
    for prefix, dep, suffix in ctx.attrs.configure_arg_store_paths:
        args.add("--configure-arg={}{}{}".format(
            prefix,
            dep[PkgsPackageInfo].logical_store_path,
            suffix,
        ))
    for prefix, suffix in ctx.attrs.configure_arg_self_store_paths:
        args.add("--configure-arg={}{}{}".format(prefix, self_store_path, suffix))
    for prefix, deps, separator, suffix in ctx.attrs.configure_arg_store_path_joins:
        args.add("--configure-arg={}{}{}".format(
            prefix,
            separator.join([dep[PkgsPackageInfo].logical_store_path for dep in deps]),
            suffix,
        ))
    for env in ctx.attrs.configure_env:
        args.add("--configure-env={}".format(env))
    for prefix, dep, suffix in ctx.attrs.configure_env_store_paths:
        args.add("--configure-env={}{}{}".format(
            prefix,
            dep[PkgsPackageInfo].logical_store_path,
            suffix,
        ))
    for prefix, suffix in ctx.attrs.configure_env_self_store_paths:
        args.add("--configure-env={}{}{}".format(prefix, self_store_path, suffix))
    for prefix, deps, separator, suffix in ctx.attrs.configure_env_store_path_joins:
        args.add("--configure-env={}{}{}".format(
            prefix,
            separator.join([dep[PkgsPackageInfo].logical_store_path for dep in deps]),
            suffix,
        ))
    for arg in ctx.attrs.make_args:
        args.add("--make-arg={}".format(arg))
    for arg in ctx.attrs.install_args:
        args.add("--install-arg={}".format(arg))
    for patch in ctx.attrs.patches:
        args.add("--patch", patch[DefaultInfo].default_outputs[0])
    args.add("--patch-strip", str(ctx.attrs.patch_strip))
    for link, target in sorted(ctx.attrs.symlinks.items()):
        args.add("--symlink", "{}={}".format(link, target))
    return args

def _pkgs_configure_make_install_package_impl(ctx):
    source = ctx.attrs.source[DefaultInfo].default_outputs[0]
    native_build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.native_build_inputs]
    build_inputs = [dep[PkgsPackageInfo] for dep in ctx.attrs.build_inputs]
    metadata = _package_metadata(ctx)

    native_runtime_store_outputs = [
        output
        for dep in native_build_inputs
        for output in dep.runtime_store_outputs
    ]

    ctx.actions.run(
        cmd_args(
            _configure_make_install_args(
                ctx,
                source,
                metadata.store_output.as_output(),
                metadata,
                native_build_inputs,
            ),
            hidden = native_runtime_store_outputs + [dep.store_output for dep in build_inputs],
        ),
        category = "pkgs_configure_make_install",
        identifier = ctx.label.name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        cmd_args(
            _configure_make_install_args(
                ctx,
                source,
                replay_tree.as_output(),
                metadata,
                native_build_inputs,
            ),
            hidden = native_runtime_store_outputs + [dep.store_output for dep in build_inputs],
        ),
        category = "pkgs_replay_configure_make_install",
        identifier = ctx.label.name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
    )

_pkgs_configure_make_install_package = rule(
    impl = _pkgs_configure_make_install_package_impl,
    attrs = {
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "configure_arg_store_paths": attrs.list(
            attrs.tuple(
                attrs.string(),
                attrs.dep(providers = [PkgsPackageInfo]),
                attrs.string(),
            ),
            default = [],
        ),
        "configure_arg_self_store_paths": attrs.list(
            attrs.tuple(attrs.string(), attrs.string()),
            default = [],
        ),
        "configure_arg_store_path_joins": attrs.list(
            attrs.tuple(
                attrs.string(),
                attrs.list(attrs.dep(providers = [PkgsPackageInfo])),
                attrs.string(),
                attrs.string(),
            ),
            default = [],
        ),
        "configure_args": attrs.list(attrs.string(), default = []),
        "configure_env": attrs.list(attrs.string(), default = []),
        "configure_env_store_paths": attrs.list(
            attrs.tuple(
                attrs.string(),
                attrs.dep(providers = [PkgsPackageInfo]),
                attrs.string(),
            ),
            default = [],
        ),
        "configure_env_self_store_paths": attrs.list(
            attrs.tuple(attrs.string(), attrs.string()),
            default = [],
        ),
        "configure_env_store_path_joins": attrs.list(
            attrs.tuple(
                attrs.string(),
                attrs.list(attrs.dep(providers = [PkgsPackageInfo])),
                attrs.string(),
                attrs.string(),
            ),
            default = [],
        ),
        "foreign": attrs.bool(default = False),
        "install_args": attrs.list(attrs.string(), default = []),
        "make_args": attrs.list(attrs.string(), default = []),
        "make_jobs": attrs.int(default = DEFAULT_MAKE_JOBS),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "out_of_source": attrs.bool(default = False),
        "output": attrs.string(default = "out"),
        "package_name": attrs.string(),
        "patch_digests": attrs.list(attrs.string(), default = []),
        "patch_strip": attrs.int(default = 1),
        "patches": attrs.list(attrs.dep(providers = [DefaultInfo]), default = []),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "source": attrs.dep(providers = [DefaultInfo]),
        "source_digests": attrs.list(attrs.string(), default = []),
        "symlinks": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_configure_make_install",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def _cc_wrapper_args(ctx, output, cc, bintools, headers, libc, shell):
    return cmd_args([
        ctx.attrs._builder[RunInfo],
        "--output",
        output,
        "--shell",
        "{}/bin/bash".format(shell.logical_store_path),
        "--cc",
        "{}/bin/gcc".format(cc.logical_store_path),
        "--cxx",
        "{}/bin/g++".format(cc.logical_store_path),
        "--cpp",
        "{}/bin/cpp".format(cc.logical_store_path),
        "--compiler-root",
        cc.logical_store_path,
        "--libc",
        libc.logical_store_path,
        "--bintools",
        bintools.logical_store_path,
        "--headers",
        headers.logical_store_path,
        "--dynamic-linker",
        "{}/{}".format(libc.logical_store_path, ctx.attrs.dynamic_linker),
    ])

def _pkgs_cc_wrapper_package_impl(ctx):
    cc = ctx.attrs.cc[PkgsPackageInfo]
    bintools = ctx.attrs.bintools[PkgsPackageInfo]
    headers = ctx.attrs.headers[PkgsPackageInfo]
    libc = ctx.attrs.libc[PkgsPackageInfo]
    shell = ctx.attrs.shell[PkgsPackageInfo]
    metadata = _package_metadata(ctx)

    ctx.actions.run(
        _cc_wrapper_args(
            ctx,
            metadata.store_output.as_output(),
            cc,
            bintools,
            headers,
            libc,
            shell,
        ),
        category = "pkgs_cc_wrapper_tree",
        identifier = ctx.label.name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        _cc_wrapper_args(ctx, replay_tree.as_output(), cc, bintools, headers, libc, shell),
        category = "pkgs_replay_cc_wrapper_tree",
        identifier = ctx.label.name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
    )

_pkgs_cc_wrapper_package = rule(
    impl = _pkgs_cc_wrapper_package_impl,
    attrs = {
        "bintools": attrs.dep(providers = [PkgsPackageInfo]),
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "cc": attrs.dep(providers = [PkgsPackageInfo]),
        "dynamic_linker": attrs.string(),
        "foreign": attrs.bool(default = False),
        "headers": attrs.dep(providers = [PkgsPackageInfo]),
        "libc": attrs.dep(providers = [PkgsPackageInfo]),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "bin"),
        "package_name": attrs.string(),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "shell": attrs.dep(providers = [PkgsPackageInfo]),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_cc_wrapper_tree",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def _bintools_wrapper_args(ctx, output, bintools, shell):
    return cmd_args([
        ctx.attrs._builder[RunInfo],
        "--output",
        output,
        "--shell",
        "{}/bin/bash".format(shell.logical_store_path),
        "--binutils",
        bintools.logical_store_path,
    ])

def _pkgs_bintools_wrapper_package_impl(ctx):
    bintools = ctx.attrs.bintools[PkgsPackageInfo]
    shell = ctx.attrs.shell[PkgsPackageInfo]
    metadata = _package_metadata(ctx)

    ctx.actions.run(
        _bintools_wrapper_args(ctx, metadata.store_output.as_output(), bintools, shell),
        category = "pkgs_bintools_wrapper_tree",
        identifier = ctx.label.name,
    )

    replay_tree = ctx.actions.declare_output("{}.replay".format(ctx.label.name), dir = True)
    ctx.actions.run(
        _bintools_wrapper_args(ctx, replay_tree.as_output(), bintools, shell),
        category = "pkgs_replay_bintools_wrapper_tree",
        identifier = ctx.label.name,
    )
    reproducibility_stamp = _declare_reproducibility_stamp(ctx, metadata, replay_tree)
    archive_metadata_stamp = _declare_archive_metadata_stamp(ctx, metadata)

    return _package_result(
        ctx,
        metadata.store_output,
        metadata,
        reproducibility_stamp,
        archive_metadata_stamp,
    )

_pkgs_bintools_wrapper_package = rule(
    impl = _pkgs_bintools_wrapper_package_impl,
    attrs = {
        "bintools": attrs.dep(providers = [PkgsPackageInfo]),
        "build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "builder": attrs.string(),
        "foreign": attrs.bool(default = False),
        "native_build_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "output": attrs.string(default = "bin"),
        "package_name": attrs.string(),
        "runtime_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "shell": attrs.dep(providers = [PkgsPackageInfo]),
        "target_system": attrs.string(),
        "target_inputs": attrs.list(attrs.dep(providers = [PkgsPackageInfo]), default = []),
        "version": attrs.string(),
        "_builder": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_bintools_wrapper_tree",
                providers = [RunInfo],
            ),
        ),
        "_reproducibility_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_reproducible_tree",
                providers = [RunInfo],
            ),
        ),
        "_archive_metadata_verifier": attrs.default_only(
            attrs.exec_dep(
                default = "//crates/pkgs-tool:pkgs_verify_archive_metadata",
                providers = [RunInfo],
            ),
        ),
    },
)

def _validate_make_jobs(name, make_jobs):
    if make_jobs < 1:
        fail("package {} must request at least one make job".format(name))

def pkgs_make_install_package(
        name,
        package_name,
        version,
        sources,
        make_args = [],
        make_jobs = DEFAULT_MAKE_JOBS,
        install_args = [],
        patches = [],
        patch_strip = 1,
        symlinks = {},
        output = "out",
        native_build_inputs = [],
        build_inputs = [],
        runtime_inputs = [],
        visibility = []):
    _validate_make_jobs(name, make_jobs)
    source = _source_set(name, sources)
    _pkgs_make_install_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "make-install-v6",
        source = source.dep,
        make_args = make_args,
        make_jobs = make_jobs,
        install_args = install_args,
        patches = _select_aware(patches, _descriptor_deps),
        patch_digests = _select_aware(patches, _descriptor_digests),
        patch_strip = patch_strip,
        symlinks = symlinks,
        source_digests = source.digests,
        target_system = _TARGET_SYSTEM,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        target_inputs = [],
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def pkgs_configure_make_install_package(
        name,
        package_name,
        version,
        sources,
        configure_args = [],
        configure_env = [],
        out_of_source = False,
        make_args = [],
        make_jobs = DEFAULT_MAKE_JOBS,
        install_args = [],
        patches = [],
        patch_strip = 1,
        symlinks = {},
        output = "out",
        native_build_inputs = [],
        build_inputs = [],
        runtime_inputs = [],
        visibility = []):
    _validate_make_jobs(name, make_jobs)
    source = _source_set(name, sources)
    lowered_configure_args = _lower_configure_values(configure_args)
    lowered_configure_env = _lower_configure_values(configure_env)
    _pkgs_configure_make_install_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "configure-make-install-v6",
        source = source.dep,
        configure_args = lowered_configure_args.strings,
        configure_arg_store_paths = lowered_configure_args.store_paths,
        configure_arg_self_store_paths = lowered_configure_args.self_store_paths,
        configure_arg_store_path_joins = lowered_configure_args.store_path_joins,
        configure_env = lowered_configure_env.strings,
        configure_env_store_paths = lowered_configure_env.store_paths,
        configure_env_self_store_paths = lowered_configure_env.self_store_paths,
        configure_env_store_path_joins = lowered_configure_env.store_path_joins,
        out_of_source = out_of_source,
        make_args = make_args,
        make_jobs = make_jobs,
        install_args = install_args,
        patches = _select_aware(patches, _descriptor_deps),
        patch_digests = _select_aware(patches, _descriptor_digests),
        patch_strip = patch_strip,
        symlinks = symlinks,
        source_digests = source.digests,
        target_system = _TARGET_SYSTEM,
        native_build_inputs = native_build_inputs,
        build_inputs = build_inputs,
        target_inputs = [],
        runtime_inputs = runtime_inputs,
        visibility = visibility,
    )

def pkgs_linux_headers_package(
        name,
        package_name,
        version,
        sources,
        kernel_release,
        make_args = [],
        make_jobs = DEFAULT_MAKE_JOBS,
        output = "out",
        native_build_inputs = [],
        visibility = []):
    _validate_make_jobs(name, make_jobs)
    source = _source_set(name, sources)
    _pkgs_linux_headers_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "linux-headers-install-v2",
        source = source.dep,
        kernel_release = kernel_release,
        make_args = make_args,
        make_jobs = make_jobs,
        source_digests = source.digests,
        target_system = _TARGET_SYSTEM,
        native_build_inputs = native_build_inputs,
        visibility = visibility,
    )

def pkgs_cc_wrapper_package(
        name,
        package_name,
        version,
        cc,
        bintools,
        headers,
        libc,
        shell,
        output = "bin",
        visibility = []):
    _pkgs_cc_wrapper_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "cc-wrapper-tree-v1",
        cc = cc,
        bintools = bintools,
        dynamic_linker = _DYNAMIC_LINKER,
        headers = headers,
        libc = libc,
        shell = shell,
        target_system = _TARGET_SYSTEM,
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
        bintools,
        shell,
        output = "bin",
        visibility = []):
    _pkgs_bintools_wrapper_package(
        name = name,
        package_name = package_name,
        version = version,
        output = output,
        builder = "bintools-wrapper-tree-v0",
        bintools = bintools,
        shell = shell,
        target_system = _TARGET_SYSTEM,
        runtime_inputs = [
            bintools,
            shell,
        ],
        visibility = visibility,
    )
