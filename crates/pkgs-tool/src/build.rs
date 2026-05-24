#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::common;

const NORMALIZED_WORK_DIR_PLACEHOLDER: &str = "@PKGS_WORK_DIR@";

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("invalid environment assignment, expected KEY=VALUE: {0}")]
    InvalidEnvAssignment(String),

    #[error("invalid symlink assignment, expected LINK=TARGET: {0}")]
    InvalidSymlinkAssignment(String),

    #[error("path must be relative and stay within the output tree: {0}")]
    InvalidRelativePath(PathBuf),

    #[error("install prefix must be absolute: {0}")]
    InvalidInstallPrefix(PathBuf),

    #[error("python bytecode directories require exactly one interpreter")]
    InvalidPythonBytecodeInterpreter,

    #[error("python bytecode optimization level must be in 0..=2: {0}")]
    InvalidPythonBytecodeOptimization(u8),

    #[error("invalid named path assignment, expected NAME=PATH: {0}")]
    InvalidNamedPathAssignment(String),

    #[error("split output {0} has no declared logical install prefix")]
    MissingSplitOutputPrefix(String),

    #[error("split output path is declared without an output destination: {0}")]
    MissingSplitOutput(String),

    #[error("split output path is missing from the staged installation: {0}")]
    MissingSplitPath(PathBuf),

    #[error("primary output path is missing from the staged installation: {0}")]
    MissingOutputPath(PathBuf),

    #[error("split output paths overlap: {0} and {1}")]
    OverlappingSplitPaths(PathBuf, PathBuf),

    #[error("primary output paths overlap: {0} and {1}")]
    OverlappingOutputPaths(PathBuf, PathBuf),

    #[error("excluded file suffix must not be empty")]
    EmptyExcludedFileSuffix,

    #[error("failed to publish sealed output from {from} to {to}: {source}")]
    PublishOutput {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitOutput {
    pub(crate) name: String,
    pub(crate) output: PathBuf,
    pub(crate) install_prefix: PathBuf,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) fn parse_env_assignments(
    assignments: &[String],
) -> Result<Vec<(String, String)>, Error> {
    assignments
        .iter()
        .map(|assignment| {
            assignment
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .ok_or_else(|| Error::InvalidEnvAssignment(assignment.clone()))
        })
        .collect()
}

pub(crate) fn create_symlinks(output: &Path, assignments: &[String]) -> Result<(), Error> {
    for assignment in assignments {
        let (link, target) = assignment
            .split_once('=')
            .ok_or_else(|| Error::InvalidSymlinkAssignment(assignment.clone()))?;
        let link = PathBuf::from(link);
        let target = PathBuf::from(target);
        validate_relative_path(&link)?;
        validate_relative_path(&target)?;

        let link_path = output.join(&link);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        symlink(&target, &link_path).map_err(|source| common::Error::CreateSymlink {
            from: target,
            to: link_path,
            source,
        })?;
    }

    Ok(())
}

pub(crate) fn apply_patches(
    source_dir: &Path,
    path: &std::ffi::OsStr,
    patches: &[PathBuf],
    patch_strip: u8,
) -> Result<(), Error> {
    for patch in patches {
        let patch = common::canonicalize(patch)?;
        common::run_command(
            common::reproducible_command("patch")
                .current_dir(source_dir)
                .env("PATH", path)
                .arg(format!("-p{patch_strip}"))
                .arg("-i")
                .arg(patch),
            "patch",
        )?;
    }

    Ok(())
}

pub(crate) fn copy_staged_prefix(
    install_root: &Path,
    install_prefix: &Path,
    output: &Path,
) -> Result<(), Error> {
    let staged_prefix = staged_prefix(install_root, install_prefix)?;
    common::copy_tree(&staged_prefix, output)?;
    Ok(())
}

pub(crate) fn parse_split_outputs(
    output_assignments: &[String],
    install_prefix_assignments: &[String],
    path_assignments: &[String],
) -> Result<Vec<SplitOutput>, Error> {
    let mut outputs = std::collections::BTreeMap::new();
    for assignment in output_assignments {
        let (name, output) = parse_named_path(assignment)?;
        outputs.insert(name, output);
    }

    let mut install_prefixes = std::collections::BTreeMap::new();
    for assignment in install_prefix_assignments {
        let (name, install_prefix) = parse_named_path(assignment)?;
        install_prefixes.insert(name, install_prefix);
    }

    let mut paths: std::collections::BTreeMap<String, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for assignment in path_assignments {
        let (name, path) = parse_named_path(assignment)?;
        validate_relative_path(&path)?;
        paths.entry(name).or_default().push(path);
    }

    let mut split_outputs = Vec::new();
    for (name, output) in outputs {
        let install_prefix = install_prefixes
            .remove(&name)
            .ok_or_else(|| Error::MissingSplitOutputPrefix(name.clone()))?;
        split_outputs.push(SplitOutput {
            paths: paths.remove(&name).unwrap_or_default(),
            install_prefix,
            name,
            output,
        });
    }
    if let Some((name, _)) = paths.into_iter().next() {
        return Err(Error::MissingSplitOutput(name));
    }
    if let Some((name, _)) = install_prefixes.into_iter().next() {
        return Err(Error::MissingSplitOutput(name));
    }
    validate_split_paths(&split_outputs)?;
    Ok(split_outputs)
}

pub(crate) fn copy_split_staged_prefix(
    install_root: &Path,
    install_prefix: &Path,
    output: &Path,
    output_paths: &[PathBuf],
    split_outputs: &[SplitOutput],
) -> Result<(), Error> {
    let staged_prefix = staged_prefix(install_root, install_prefix)?;
    if output_paths.is_empty() {
        common::copy_tree(&staged_prefix, output)?;
    } else {
        validate_output_paths(output_paths)?;
        fs::create_dir_all(output).map_err(|source| common::Error::CreateDir {
            path: output.to_path_buf(),
            source,
        })?;
        for relative in output_paths {
            validate_relative_path(relative)?;
            let source = staged_prefix.join(relative);
            if fs::symlink_metadata(&source).is_err() {
                return Err(Error::MissingOutputPath(source));
            }
            let destination = output.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            common::copy_tree(&source, &destination)?;
        }
    }
    for split_output in split_outputs {
        fs::create_dir_all(&split_output.output).map_err(|source| common::Error::CreateDir {
            path: split_output.output.clone(),
            source,
        })?;
        for relative in &split_output.paths {
            let primary_source = output.join(relative);
            let source = if fs::symlink_metadata(&primary_source).is_ok() {
                primary_source
            } else {
                staged_prefix.join(relative)
            };
            if fs::symlink_metadata(&source).is_err() {
                return Err(Error::MissingSplitPath(source));
            }
            let destination = split_output.output.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            common::copy_tree(&source, &destination)?;
            if source.starts_with(output) {
                remove_tree(&source)?;
            }
        }
    }
    rewrite_split_pkg_config_paths(output, install_prefix, split_outputs)?;
    Ok(())
}

pub(crate) fn exclude_file_suffixes(output: &Path, suffixes: &[String]) -> Result<(), Error> {
    if suffixes.iter().any(|suffix| suffix.is_empty()) {
        return Err(Error::EmptyExcludedFileSuffix);
    }
    if suffixes.is_empty() {
        return Ok(());
    }
    exclude_file_suffixes_in_tree(output, suffixes)
}

fn exclude_file_suffixes_in_tree(path: &Path, suffixes: &[String]) -> Result<(), Error> {
    for entry in common::sorted_dir_entries(path)? {
        let entry_path = entry.path();
        let metadata =
            fs::symlink_metadata(&entry_path).map_err(|source| common::Error::Metadata {
                path: entry_path.clone(),
                source,
            })?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            exclude_file_suffixes_in_tree(&entry_path, suffixes)?;
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if suffixes.iter().any(|suffix| file_name.ends_with(suffix)) {
            fs::remove_file(&entry_path).map_err(|source| common::Error::RemoveFile {
                path: entry_path,
                source,
            })?;
        }
    }
    Ok(())
}

fn staged_prefix(install_root: &Path, install_prefix: &Path) -> Result<PathBuf, Error> {
    let relative_prefix = install_prefix
        .strip_prefix("/")
        .map_err(|_| Error::InvalidInstallPrefix(install_prefix.to_path_buf()))?;
    Ok(install_root.join(relative_prefix))
}

pub(crate) fn staging_outputs(
    work_dir: &Path,
    split_outputs: &[SplitOutput],
) -> (PathBuf, Vec<SplitOutput>) {
    let output = work_dir.join("sealed-output");
    let split_outputs = split_outputs
        .iter()
        .map(|split_output| SplitOutput {
            name: split_output.name.clone(),
            output: work_dir.join(format!("sealed-output-{}", split_output.name)),
            install_prefix: split_output.install_prefix.clone(),
            paths: split_output.paths.clone(),
        })
        .collect();
    (output, split_outputs)
}

pub(crate) fn publish_sealed_outputs(
    output: &Path,
    destination: &Path,
    split_outputs: &[SplitOutput],
    split_destinations: &[SplitOutput],
) -> Result<(), Error> {
    publish_sealed_output(output, destination)?;
    for (split_output, split_destination) in split_outputs.iter().zip(split_destinations) {
        debug_assert_eq!(split_output.name, split_destination.name);
        publish_sealed_output(&split_output.output, &split_destination.output)?;
    }
    Ok(())
}

fn publish_sealed_output(output: &Path, destination: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(output).map_err(|source| common::Error::Metadata {
        path: output.to_path_buf(),
        source,
    })?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o200);
    fs::set_permissions(output, permissions).map_err(|source| common::Error::SetPermissions {
        path: output.to_path_buf(),
        source,
    })?;
    fs::rename(output, destination).map_err(|source| Error::PublishOutput {
        from: output.to_path_buf(),
        to: destination.to_path_buf(),
        source,
    })?;
    let metadata = fs::symlink_metadata(destination).map_err(|source| common::Error::Metadata {
        path: destination.to_path_buf(),
        source,
    })?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(destination, permissions).map_err(|source| {
        common::Error::SetPermissions {
            path: destination.to_path_buf(),
            source,
        }
    })?;
    Ok(())
}

fn parse_named_path(assignment: &str) -> Result<(String, PathBuf), Error> {
    let (name, path) = assignment
        .split_once('=')
        .ok_or_else(|| Error::InvalidNamedPathAssignment(assignment.to_owned()))?;
    if name.is_empty() || path.is_empty() {
        return Err(Error::InvalidNamedPathAssignment(assignment.to_owned()));
    }
    Ok((name.to_owned(), PathBuf::from(path)))
}

fn validate_split_paths(split_outputs: &[SplitOutput]) -> Result<(), Error> {
    let paths = split_outputs
        .iter()
        .flat_map(|output| output.paths.iter())
        .collect::<Vec<_>>();
    for (index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(Error::OverlappingSplitPaths(
                    (*left).clone(),
                    (*right).clone(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_output_paths(paths: &[PathBuf]) -> Result<(), Error> {
    for (index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(Error::OverlappingOutputPaths(
                    (*left).clone(),
                    (*right).clone(),
                ));
            }
        }
    }
    Ok(())
}

fn remove_tree(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|source| common::Error::RemoveDir {
            path: path.to_path_buf(),
            source,
        })?;
    } else {
        fs::remove_file(path).map_err(|source| common::Error::RemoveFile {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn rewrite_split_pkg_config_paths(
    primary_output: &Path,
    primary_install_prefix: &Path,
    split_outputs: &[SplitOutput],
) -> Result<(), Error> {
    rewrite_pkg_config_paths_in_tree(primary_output, primary_install_prefix, split_outputs)?;
    for split_output in split_outputs {
        rewrite_pkg_config_paths_in_tree(
            &split_output.output,
            primary_install_prefix,
            split_outputs,
        )?;
    }
    Ok(())
}

fn rewrite_pkg_config_paths_in_tree(
    path: &Path,
    primary_install_prefix: &Path,
    split_outputs: &[SplitOutput],
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            rewrite_pkg_config_paths_in_tree(&entry.path(), primary_install_prefix, split_outputs)?;
        }
        return Ok(());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("pc") {
        return Ok(());
    }

    let contents = fs::read_to_string(path).map_err(|source| common::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let rewritten = rewrite_pkg_config_variables(&contents, primary_install_prefix, split_outputs);
    if rewritten == contents {
        return Ok(());
    }
    fs::write(path, rewritten).map_err(|source| common::Error::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    common::preserve_metadata(&metadata, path)?;
    Ok(())
}

fn rewrite_pkg_config_variables(
    contents: &str,
    primary_install_prefix: &Path,
    split_outputs: &[SplitOutput],
) -> String {
    let directories = [
        ("includedir=", "include"),
        ("libdir=", "lib"),
        ("sharedlibdir=", "lib"),
        ("datadir=", "share"),
        ("datarootdir=", "share"),
    ];
    let mut rewritten = String::with_capacity(contents.len());
    for line in contents.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map(|body| (body, "\n"))
            .unwrap_or((line, ""));
        let replacement = directories.iter().find_map(|(variable, relative)| {
            body.strip_prefix(variable).and_then(|_| {
                output_prefix_for_dir(Path::new(relative), split_outputs).and_then(|prefix| {
                    (prefix != primary_install_prefix)
                        .then(|| format!("{variable}{}/{}", prefix.display(), relative))
                })
            })
        });
        rewritten.push_str(replacement.as_deref().unwrap_or(body));
        rewritten.push_str(newline);
    }
    rewritten
}

fn output_prefix_for_dir<'a>(
    relative: &Path,
    split_outputs: &'a [SplitOutput],
) -> Option<&'a Path> {
    split_outputs.iter().find_map(|output| {
        output
            .paths
            .iter()
            .any(|path| relative.starts_with(path))
            .then_some(output.install_prefix.as_path())
    })
}

pub(crate) fn compile_python_bytecode(
    output: &Path,
    install_prefix: &Path,
    interpreter: Option<&Path>,
    self_interpreter: Option<&Path>,
    directories: &[PathBuf],
    optimization_levels: &[u8],
) -> Result<(), Error> {
    if directories.is_empty() {
        return Ok(());
    }

    let interpreter = match (interpreter, self_interpreter) {
        (Some(interpreter), None) => interpreter.to_path_buf(),
        (None, Some(interpreter)) => {
            validate_relative_path(interpreter)?;
            output.join(interpreter)
        }
        _ => return Err(Error::InvalidPythonBytecodeInterpreter),
    };

    for level in optimization_levels {
        if *level > 2 {
            return Err(Error::InvalidPythonBytecodeOptimization(*level));
        }
    }

    for directory in directories {
        validate_relative_path(directory)?;
        let directory = output.join(directory);
        for level in optimization_levels {
            common::run_command(
                common::reproducible_command(&interpreter)
                    .arg("-m")
                    .arg("compileall")
                    .arg("-q")
                    .arg("-f")
                    .arg("--invalidation-mode")
                    .arg("checked-hash")
                    .arg("-s")
                    .arg(output)
                    .arg("-p")
                    .arg(install_prefix)
                    .arg("-o")
                    .arg(level.to_string())
                    .arg(&directory),
                &interpreter.display().to_string(),
            )?;
        }
    }

    Ok(())
}

pub(crate) fn sanitize_libtool_archives(output: &Path, work_dir: &Path) -> Result<(), Error> {
    sanitize_libtool_archives_in_tree(output, work_dir)
}

pub(crate) fn normalize_work_dir_text_paths(
    output: &Path,
    work_dir: &Path,
    relative_paths: &[PathBuf],
) -> Result<(), Error> {
    let work_dir = work_dir.to_string_lossy();
    for relative_path in relative_paths {
        validate_relative_path(relative_path)?;
        let path = output.join(relative_path);
        if !path.exists() {
            continue;
        }

        let metadata = fs::symlink_metadata(&path).map_err(|source| common::Error::Metadata {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }

        let contents = fs::read_to_string(&path).map_err(|source| common::Error::ReadFile {
            path: path.clone(),
            source,
        })?;
        if !contents.contains(work_dir.as_ref()) {
            continue;
        }

        let rewritten = contents.replace(work_dir.as_ref(), NORMALIZED_WORK_DIR_PLACEHOLDER);
        fs::write(&path, rewritten).map_err(|source| common::Error::WriteFile {
            path: path.clone(),
            source,
        })?;
        common::preserve_metadata(&metadata, &path)?;
    }

    Ok(())
}

pub(crate) fn sanitize_self_referential_linker_scripts(
    output: &Path,
    install_prefix: &Path,
) -> Result<(), Error> {
    sanitize_self_referential_linker_scripts_in_tree(output, output, install_prefix)
}

fn sanitize_libtool_archives_in_tree(path: &Path, work_dir: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            sanitize_libtool_archives_in_tree(&entry.path(), work_dir)?;
        }
        return Ok(());
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("la") {
        return Ok(());
    }

    let contents = fs::read_to_string(path).map_err(|source| common::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(rewritten) = rewrite_libtool_archive(&contents, work_dir) else {
        return Ok(());
    };

    fs::write(path, rewritten).map_err(|source| common::Error::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    common::preserve_metadata(&metadata, path)?;
    Ok(())
}

fn sanitize_self_referential_linker_scripts_in_tree(
    path: &Path,
    output: &Path,
    install_prefix: &Path,
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            sanitize_self_referential_linker_scripts_in_tree(
                &entry.path(),
                output,
                install_prefix,
            )?;
        }
        return Ok(());
    }

    if !is_linker_script_candidate(path) {
        return Ok(());
    }

    let bytes = fs::read(path).map_err(|source| common::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let Ok(contents) = String::from_utf8(bytes) else {
        return Ok(());
    };
    let Some(rewritten) =
        rewrite_self_referential_linker_script(&contents, path, output, install_prefix)
    else {
        return Ok(());
    };

    fs::write(path, rewritten).map_err(|source| common::Error::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    common::preserve_metadata(&metadata, path)?;
    Ok(())
}

fn rewrite_libtool_archive(contents: &str, work_dir: &Path) -> Option<String> {
    let mut changed = false;
    let mut rewritten = String::with_capacity(contents.len());

    for line in contents.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map(|body| (body, "\n"))
            .unwrap_or((line, ""));
        if let Some(updated) = rewrite_dependency_libs_line(body, work_dir) {
            changed = true;
            rewritten.push_str(&updated);
        } else {
            rewritten.push_str(body);
        }
        rewritten.push_str(newline);
    }

    changed.then_some(rewritten)
}

fn rewrite_dependency_libs_line(line: &str, work_dir: &Path) -> Option<String> {
    let dependencies = line
        .strip_prefix("dependency_libs='")
        .and_then(|line| line.strip_suffix('\''))?;
    let work_dir = work_dir.to_string_lossy();
    let filtered: Vec<_> = dependencies
        .split_whitespace()
        .filter(|token| !is_transient_lib_search_path(token, &work_dir))
        .collect();
    let original: Vec<_> = dependencies.split_whitespace().collect();
    if filtered == original {
        return None;
    }

    if filtered.is_empty() {
        return Some("dependency_libs=''".to_owned());
    }

    Some(format!("dependency_libs=' {}'", filtered.join(" ")))
}

fn is_transient_lib_search_path(token: &str, work_dir: &str) -> bool {
    token
        .strip_prefix("-L")
        .is_some_and(|path| Path::new(path).starts_with(work_dir))
}

fn is_linker_script_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("a" | "so")
    )
}

fn rewrite_self_referential_linker_script(
    contents: &str,
    script_path: &Path,
    output: &Path,
    install_prefix: &Path,
) -> Option<String> {
    if !contents.trim_start().starts_with("/* GNU ld script") {
        return None;
    }

    let script_dir = script_path.parent()?;
    let relative_script_dir = script_dir.strip_prefix(output).ok()?;
    let install_dir = install_prefix.join(relative_script_dir);
    let install_dir = install_dir.to_string_lossy();
    // Implicit linker scripts resolve bare filenames from the script directory,
    // which works both inside a sysroot and during non-sysroot bootstrap links.
    let sibling_prefix = format!("{install_dir}/");

    contents
        .contains(&sibling_prefix)
        .then(|| contents.replace(&sibling_prefix, ""))
}

fn validate_relative_path(path: &Path) -> Result<(), Error> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidRelativePath(path.to_path_buf()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_environment_assignments() {
        assert_eq!(
            parse_env_assignments(&["CC=cc".to_owned(), "AR=ar".to_owned()]).unwrap(),
            vec![
                ("CC".to_owned(), "cc".to_owned()),
                ("AR".to_owned(), "ar".to_owned()),
            ]
        );
        assert!(parse_env_assignments(&["BROKEN".to_owned()]).is_err());
    }

    #[test]
    fn rejects_non_relative_output_paths() {
        assert!(validate_relative_path(Path::new("bin/sh")).is_ok());
        assert!(validate_relative_path(Path::new("/bin/sh")).is_err());
        assert!(validate_relative_path(Path::new("../bin/sh")).is_err());
    }

    #[test]
    fn excludes_files_by_suffix_from_published_trees() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let nested = output.join("lib/modules");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("runtime.pm"), "runtime").unwrap();
        fs::write(nested.join("guide.pod"), "documentation").unwrap();

        exclude_file_suffixes(&output, &[".pod".to_owned()]).unwrap();

        assert!(nested.join("runtime.pm").exists());
        assert!(!nested.join("guide.pod").exists());
        assert!(exclude_file_suffixes(&output, &["".to_owned()]).is_err());
    }

    #[test]
    fn strips_transient_build_search_paths_from_libtool_archives() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let libdir = output.join("lib");
        let work_dir = temp.path().join("work");
        fs::create_dir_all(&libdir).unwrap();
        fs::create_dir_all(&work_dir).unwrap();

        let archive = libdir.join("libbfd.la");
        fs::write(
            &archive,
            format!(
                "dependency_libs=' -L{}/source/zlib -lz /pkgs/store/example/lib/libgmp.la'\n",
                work_dir.display()
            ),
        )
        .unwrap();

        sanitize_libtool_archives(&output, &work_dir).unwrap();

        assert_eq!(
            fs::read_to_string(&archive).unwrap(),
            "dependency_libs=' -lz /pkgs/store/example/lib/libgmp.la'\n",
        );
    }

    #[test]
    fn normalizes_declared_text_paths_that_retain_the_work_directory() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let work_dir = temp.path().join("work");
        let generated = output.join("lib/pgxs/src/Makefile.global");
        fs::create_dir_all(generated.parent().unwrap()).unwrap();
        fs::create_dir_all(&work_dir).unwrap();
        fs::write(
            &generated,
            format!("abs_top_builddir = {}/build\n", work_dir.display()),
        )
        .unwrap();

        normalize_work_dir_text_paths(
            &output,
            &work_dir,
            &[PathBuf::from("lib/pgxs/src/Makefile.global")],
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(generated).unwrap(),
            "abs_top_builddir = @PKGS_WORK_DIR@/build\n",
        );
    }

    #[test]
    fn strips_same_directory_store_prefixes_from_linker_scripts() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let libdir = output.join("lib");
        fs::create_dir_all(&libdir).unwrap();

        let libc = libdir.join("libc.so");
        fs::write(
            &libc,
            "/* GNU ld script */\nGROUP ( /pkgs/store/glibc/lib/libc.so.6 /pkgs/store/glibc/lib/libc_nonshared.a )\n",
        )
        .unwrap();

        let libm = libdir.join("libm.so");
        fs::write(
            &libm,
            "/* GNU ld script */\nGROUP ( /pkgs/store/glibc/lib/libm.so.6 AS_NEEDED ( /pkgs/store/glibc/lib/libmvec.so.1 ) )\n",
        )
        .unwrap();

        let libm_archive = libdir.join("libm.a");
        fs::write(
            &libm_archive,
            "/* GNU ld script */\nGROUP ( /pkgs/store/glibc/lib/libm-2.42.a /pkgs/store/glibc/lib/libmvec.a )\n",
        )
        .unwrap();

        sanitize_self_referential_linker_scripts(&output, Path::new("/pkgs/store/glibc")).unwrap();

        assert_eq!(
            fs::read_to_string(&libc).unwrap(),
            "/* GNU ld script */\nGROUP ( libc.so.6 libc_nonshared.a )\n",
        );
        assert_eq!(
            fs::read_to_string(&libm).unwrap(),
            "/* GNU ld script */\nGROUP ( libm.so.6 AS_NEEDED ( libmvec.so.1 ) )\n",
        );
        assert_eq!(
            fs::read_to_string(&libm_archive).unwrap(),
            "/* GNU ld script */\nGROUP ( libm-2.42.a libmvec.a )\n",
        );
    }
}
