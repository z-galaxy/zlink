use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Generate Rust code from Varlink IDL files.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Input Varlink IDL file(s).
    #[arg(value_name = "FILES", num_args = 1..)]
    pub files: Vec<PathBuf>,

    /// Output file path (defaults to stdout if not specified).
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Generate separate files for each interface (ignored if --output is specified).
    #[arg(short = 'm', long)]
    pub multiple_files: bool,

    /// Format the generated Rust code using rustfmt.
    #[arg(short = 'f', long)]
    pub rustfmt: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate code from Varlink IDL file(s).
    Generate {
        /// Input Varlink IDL file(s).
        #[arg(value_name = "FILES", num_args = 1..)]
        files: Vec<PathBuf>,

        /// Output file path (defaults to stdout if not specified).
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Generate separate files for each interface.
        #[arg(short = 'm', long)]
        multiple_files: bool,

        /// Format the generated Rust code using rustfmt.
        #[arg(short = 'f', long)]
        rustfmt: bool,
    },
}
