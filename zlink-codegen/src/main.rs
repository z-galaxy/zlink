use clap::Parser;
use zlink_codegen::{CodegenOptions, Result};

mod cli;
use cli::Args;

fn main() -> Result<()> {
    let args = Args::parse();

    // Handle the case where no command is provided (use files directly).
    let (files, output, multiple_files, rustfmt) = match args.command {
        Some(cli::Command::Generate {
            files,
            output,
            multiple_files,
            rustfmt,
        }) => (files, output, multiple_files, rustfmt),
        None => (args.files, args.output, args.multiple_files, args.rustfmt),
    };

    if files.is_empty() {
        eprintln!("Error: No input files specified");
        eprintln!("Usage: zlink-codegen <FILES>... [OPTIONS]");
        std::process::exit(1);
    }

    // Create configuration from command-line arguments
    let config = CodegenOptions {
        files,
        output,
        multiple_files,
        rustfmt,
    };

    // Generate the files
    zlink_codegen::generate_files(&config)?;

    Ok(())
}
