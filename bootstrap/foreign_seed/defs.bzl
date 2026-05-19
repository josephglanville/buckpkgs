load("//rules:pkgs.bzl", "pkgs_package")

def foreign_wrapped_package(name, commands):
    scripts = {}
    for command in commands:
        script_name = "{}__{}".format(name, command)
        native.write_file(
            name = script_name,
            out = command,
            content = [
                "#!/bin/sh",
                "exec /usr/bin/{} \"$@\"".format(command),
            ],
            is_executable = True,
        )
        scripts["bin/" + command] = ":" + script_name

    tree_name = name + "__tree"
    native.filegroup(
        name = tree_name,
        srcs = scripts,
        copy = True,
    )

    pkgs_package(
        name = name,
        package_name = name,
        version = "foreign-seed",
        output = "bin",
        builder = "foreign-seed-wrapper-v0",
        foreign = True,
        source_digests = ["sha256:" + sha256("\n".join(commands))],
        src = ":" + tree_name,
        visibility = ["PUBLIC"],
    )
