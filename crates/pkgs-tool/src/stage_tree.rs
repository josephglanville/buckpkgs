use std::path::PathBuf;

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-stage-tree")]
pub(crate) struct Args {
    #[arg(long)]
    source: PathBuf,

    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("source tree does not exist: {0}")]
    MissingSource(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    if !args.source.exists() {
        return Err(Error::MissingSource(args.source.clone()));
    }

    if !args.output.exists() {
        common::copy_tree(&args.source, &args.output)?;
        common::make_tree_read_only(&args.output)?;
    }

    Ok(())
}
