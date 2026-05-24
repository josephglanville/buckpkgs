use std::fs;
use std::path::PathBuf;

use clap::Parser;
use thiserror::Error;

use crate::{build, common};

#[derive(Debug, Parser)]
#[command(name = "pkgs-make-install")]
pub(crate) struct Args {
    #[arg(long)]
    source: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long = "output-path")]
    output_paths: Vec<PathBuf>,

    #[arg(long = "exclude-file-suffix")]
    exclude_file_suffixes: Vec<String>,

    #[arg(long)]
    preserve_debug: bool,

    #[arg(long)]
    relocate_split_metadata_prefix: bool,

    #[arg(long = "split-output")]
    split_outputs: Vec<String>,

    #[arg(long = "split-output-prefix")]
    split_output_prefixes: Vec<String>,

    #[arg(long = "split-path")]
    split_paths: Vec<String>,

    #[arg(long = "split-reference-symlink")]
    split_reference_symlinks: Vec<String>,

    #[arg(long)]
    install_prefix: PathBuf,

    #[arg(long = "path-entry")]
    path_entries: Vec<PathBuf>,

    #[arg(long = "link-input")]
    link_inputs: Vec<PathBuf>,

    #[arg(long = "link-interface-input")]
    link_interface_inputs: Vec<PathBuf>,

    #[arg(long = "pkg-config-path")]
    pkg_config_paths: Vec<PathBuf>,

    #[arg(long = "pkg-config-path-for-build")]
    pkg_config_paths_for_build: Vec<PathBuf>,

    #[arg(long = "pkg-config-path-for-target")]
    pkg_config_paths_for_target: Vec<PathBuf>,

    #[arg(long = "make-arg")]
    make_args: Vec<String>,

    #[arg(long, default_value_t = common::DEFAULT_MAKE_JOBS)]
    make_jobs: usize,

    #[arg(long = "install-arg")]
    install_args: Vec<String>,

    #[arg(long = "python-bytecode-dir")]
    python_bytecode_dirs: Vec<PathBuf>,

    #[arg(long = "python-bytecode-interpreter")]
    python_bytecode_interpreter: Option<PathBuf>,

    #[arg(long = "python-bytecode-self-interpreter")]
    python_bytecode_self_interpreter: Option<PathBuf>,

    #[arg(long = "python-bytecode-optimize")]
    python_bytecode_optimizations: Vec<u8>,

    #[arg(long = "patch")]
    patches: Vec<PathBuf>,

    #[arg(long, default_value_t = 1)]
    patch_strip: u8,

    #[arg(long = "symlink")]
    symlinks: Vec<String>,

    #[arg(long, default_value = "make")]
    make_program: String,

    #[arg(long, default_value = "PREFIX")]
    prefix_var: String,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Build(#[from] build::Error),

    #[error(transparent)]
    Common(#[from] common::Error),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    let (work_path, _temp_work) =
        if let Some(path) = common::deterministic_scratch_dir("pkgs-make-install")? {
            (path, None)
        } else {
            let temp_work = tempfile::Builder::new()
                .prefix("pkgs-make-install-")
                .tempdir()
                .map_err(|source| common::Error::CreateTempDir { source })?;
            (temp_work.path().to_path_buf(), Some(temp_work))
        };
    let source_dir = work_path.join("source");
    common::copy_tree(&args.source, &source_dir)?;

    let install_root = work_path.join("install-root");
    fs::create_dir(&install_root).map_err(|source| common::Error::CreateDir {
        path: install_root.clone(),
        source,
    })?;

    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let path = common::compiler_wrapped_path(
        &path,
        &work_path,
        &args.link_inputs,
        &args.link_interface_inputs,
    )?;
    let makeflags = common::makeflags(args.make_jobs)?;

    build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;

    let mut make_command = common::reproducible_command(&args.make_program);
    make_command
        .current_dir(&source_dir)
        .env("PATH", &path)
        .env("MAKEFLAGS", &makeflags)
        .args(&args.make_args);
    add_pkg_config_environment(&mut make_command, args)?;
    common::run_command(&mut make_command, &args.make_program)?;

    let prefix_arg = format!("{}={}", args.prefix_var, args.install_prefix.display());
    let mut install_command = common::reproducible_command(&args.make_program);
    install_command
        .current_dir(&source_dir)
        .env("PATH", &path)
        .env("MAKEFLAGS", &makeflags)
        .arg("install")
        .arg(prefix_arg)
        .arg(format!("DESTDIR={}", install_root.display()))
        .args(&args.install_args);
    add_pkg_config_environment(&mut install_command, args)?;
    common::run_command(&mut install_command, &args.make_program)?;

    let split_destinations = build::parse_split_outputs(
        &args.split_outputs,
        &args.split_output_prefixes,
        &args.split_paths,
        &args.split_reference_symlinks,
    )?;
    let (output, split_outputs) = build::staging_outputs(&work_path, &split_destinations);
    build::copy_split_staged_prefix(
        &install_root,
        &args.install_prefix,
        &output,
        &args.output_paths,
        &split_outputs,
        args.relocate_split_metadata_prefix,
    )?;
    build::exclude_file_suffixes(&output, &args.exclude_file_suffixes)?;
    build::sanitize_libtool_archives(&output, &work_path)?;
    build::sanitize_self_referential_linker_scripts(
        &output,
        &args.install_prefix,
        &args.install_prefix,
        &split_outputs,
    )?;
    for split_output in &split_outputs {
        build::exclude_file_suffixes(&split_output.output, &args.exclude_file_suffixes)?;
        build::sanitize_libtool_archives(&split_output.output, &work_path)?;
        build::sanitize_self_referential_linker_scripts(
            &split_output.output,
            &args.install_prefix,
            &split_output.install_prefix,
            &split_outputs,
        )?;
    }
    if !args.preserve_debug {
        build::strip_debug_sections(&output, &path)?;
        for split_output in &split_outputs {
            build::strip_debug_sections(&split_output.output, &path)?;
        }
    }
    let output = common::canonicalize(&output)?;
    build::create_symlinks(&output, &args.symlinks)?;
    build::compile_python_bytecode(
        &output,
        &args.install_prefix,
        args.python_bytecode_interpreter.as_deref(),
        args.python_bytecode_self_interpreter.as_deref(),
        &args.python_bytecode_dirs,
        &args.python_bytecode_optimizations,
    )?;
    for output in std::iter::once(&output).chain(split_outputs.iter().map(|output| &output.output))
    {
        common::normalize_tree_mtimes(output)?;
        common::make_tree_read_only(output)?;
    }
    build::publish_sealed_outputs(&output, &args.output, &split_outputs, &split_destinations)?;
    Ok(())
}

fn add_pkg_config_environment(
    command: &mut std::process::Command,
    args: &Args,
) -> Result<(), common::Error> {
    common::add_pkg_config_environment(
        command,
        &args.pkg_config_paths,
        &args.pkg_config_paths_for_build,
        &args.pkg_config_paths_for_target,
    )
}
