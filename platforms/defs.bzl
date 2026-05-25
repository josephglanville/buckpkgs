load("@prelude//cfg/exec_platform:marker.bzl", "get_exec_platform_marker")

def _remote_platform(ctx):
    constraints = dict()
    constraints.update(ctx.attrs.cpu_configuration[ConfigurationInfo].constraints)
    constraints.update(ctx.attrs.os_configuration[ConfigurationInfo].constraints)
    configuration = ConfigurationInfo(constraints = constraints, values = {})
    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = configuration,
        executor_config = CommandExecutorConfig(
            local_enabled = False,
            remote_enabled = True,
            remote_cache_enabled = True,
            allow_cache_uploads = True,
            remote_execution_properties = {
                "foundry.bubblewrap_runtime_profile.v1": "buckpkgs-bootstrap-v5",
            },
            remote_execution_use_case = "foundry-local",
            remote_output_paths = "output_paths",
        ),
    )
    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(ctx.label.raw_target()), configuration = configuration),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
            exec_marker_constraint = get_exec_platform_marker(),
        ),
    ]

remote_platform = rule(
    attrs = {
        "cpu_configuration": attrs.dep(providers = [ConfigurationInfo]),
        "os_configuration": attrs.dep(providers = [ConfigurationInfo]),
    },
    impl = _remote_platform,
)
