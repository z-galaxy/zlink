#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
//! Code generation for Varlink interfaces.

use anyhow::{Context, Result};
use std::{
    fs,
    path::Path,
};
use zlink::idl::Interface;

mod codegen;
pub use codegen::CodeGenerator;
mod error;
pub use self::error::Error;

/// Generate Rust code from a Varlink interface.
pub fn generate_interface(interface: &Interface<'_>) -> Result<String> {
    let mut generator = CodeGenerator::new();
    generator.generate_interface(interface, false)?;
    Ok(generator.output())
}

/// Generate Rust code from multiple Varlink interfaces.
pub fn generate_interfaces(interfaces: &[Interface<'_>]) -> Result<String> {
    let mut generator = CodeGenerator::new();

    // Add module-level header for multiple interfaces.
    if interfaces.len() > 1 {
        generator.write_module_header()?;
    }

    for interface in interfaces.iter() {
        // Skip module header for all interfaces when generating multiple.
        let skip_header = interfaces.len() > 1;
        generator.generate_interface(interface, skip_header)?;
    }
    Ok(generator.output())
}

/// Format generated Rust code using rustfmt.
pub fn format_code(code: &str) -> Result<String> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let mut child = Command::new("rustfmt")
        .arg("--edition=2021")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn rustfmt")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(code.as_bytes())
            .context("Failed to write to rustfmt stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for rustfmt")?;

    if !output.status.success() {
        // If rustfmt fails, return the original code.
        eprintln!(
            "Warning: rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(code.to_string());
    }

    String::from_utf8(output.stdout).context("Failed to parse rustfmt output")
}

/// Configuration options for Varlink code generation.
///
/// This struct provides fine-grained control over the code generation process,
/// allowing customization of output formatting and other generation behaviors.
///
/// # Examples
///
/// ## Basic usage with default options
/// ```
/// use zlink_codegen::CodegenOptions;
///
/// let options = CodegenOptions::default();
/// ```
///
/// ## Enable rustfmt formatting
/// ```
/// use zlink_codegen::CodegenOptions;
///
/// let options = CodegenOptions {
///     rustfmt: true,
///     ..Default::default()
/// };
/// ```
#[derive(Default)]
pub struct CodegenOptions {
    /// Whether to format the generated Rust code using `rustfmt`.
    ///
    /// Default value: false
    pub rustfmt: bool,
    /// The output directory for the generated file.
    pub output_directory: Option<String>,
    // TODO: Add other options for controlling the generated code. For example, an option to
    // specify the integer type (see https://github.com/zeenix/zlink/issues/109).
}

/// Generate a Rust source file from a Varlink interface file.
///
/// This function reads a Varlink interface definition file, parses it, generates
/// corresponding Rust code, and writes the output to a `.rs` file in the same directory.
/// It's designed to be used in `build.rs` scripts for compile-time code generation.
///
/// # Arguments
///
/// * `interface_file` - Path to the Varlink interface file (typically with `.varlink` extension).
/// * `config` - Code generation options, including whether to format with rustfmt.
///
/// # Output File Naming
///
/// The output file is generated in the same directory as the input file with:
/// - The same base name as the input file (without extension).
/// - Dots replaced with underscores and converted to lowercase.
///
/// For example: `com.example.Service.varlink` → `com_example_service.rs`
///
/// # Returns
///
/// Returns `Ok(true)` on successful code generation and file writing.
///
/// # Errors
///
/// This function will return an error if:
/// - `Error::InvalidArgument` - The input path is invalid or cannot be processed.
/// - `Error::CodegenFailed` - Code generation encounters an error.
/// - `Error::FormatFailed` - rustfmt formatting fails (when `config.rustfmt` is true).
/// - `Error::Io` - File I/O operations fail (reading input or writing output).
/// - `Error::Parse` - The Varlink interface definition is malformed or invalid.
///
/// # Examples
///
/// ```no_run
/// extern crate zlink_codegen;
///
/// fn main() {
///     zlink_codegen::generate_file(
///         "src/io.systemd.UserDatabase.varlink",
///         &zlink_codegen::CodegenOptions {
///             rustfmt: true,
///             ..Default::default()
///         },
///     );
/// }
/// ```
pub fn generate_file<P>(interface_file: &P, config: &CodegenOptions) -> Result<bool, Error>
where
    P: AsRef<Path> + ?Sized,
{
    let input_path = interface_file.as_ref();
    let input_path_no_ext = input_path.with_extension("");
    let output_filename = input_path_no_ext
        .file_name()
        .ok_or(Error::InvalidArgument)?
        .to_str()
        .ok_or(Error::InvalidArgument)?
        .replace('.', "_")
        .to_lowercase();
    // If no output directory is specified use the source directory.
    let output_dir: &Path = match &config.output_directory {
        Some(o) => o.as_ref(),
        None => input_path.parent().ok_or(Error::InvalidArgument)?,
    };
    let output_path = output_dir.join(Path::new(&output_filename).with_extension("rs"));

    // Read from input file
    let content = fs::read_to_string(input_path)?;

    // Parse and generate the interface
    let interface = Interface::try_from(content.as_str())?;
    let mut output = match generate_interface(&interface) {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "Failed to generate code for interface {}: {e}",
                interface.name()
            );
            return Err(Error::CodegenFailed)
        },
    };

    // Format the code
    if config.rustfmt {
        output = match format_code(&output) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Failed to format code: {e}");
                return Err(Error::FormatFailed)
            },
        }
    }

    // Write output to file.
    fs::write(&output_path, output)?;

    Ok(true)
}

/// Generate a Rust source file from multiple Varlink interface files.
///
/// This function reads multiple Varlink interface definition files, parses them,
/// generates corresponding Rust code using `generate_interfaces()`, and writes
/// the combined output to a single `.rs` file.
///
/// # Arguments
///
/// * `interface_files` - Vector of paths to Varlink interface files.
/// * `output_file` - Path where the generated Rust code will be written.
/// * `config` - Code generation options, including whether to format with rustfmt.
///
/// # Returns
///
/// Returns `Ok(true)` on successful code generation and file writing.
///
/// # Errors
///
/// This function will return an error if:
/// - `Error::InvalidArgument` - Any input path is invalid or cannot be processed.
/// - `Error::CodegenFailed` - Code generation encounters an error.
/// - `Error::FormatFailed` - rustfmt formatting fails (when `config.rustfmt` is true).
/// - `Error::Io` - File I/O operations fail (reading input or writing output).
/// - `Error::Parse` - Any Varlink interface definition is malformed or invalid.
pub fn generate_files<P>(
    interface_files: &[&P],
    output_file: &P,
    config: &CodegenOptions,
) -> Result<bool, Error>
where
    P: AsRef<Path> + ?Sized,
{
    // Read and parse all interface files
    let mut file_contents = Vec::new();
    for interface_file in interface_files {
        let content = fs::read_to_string(interface_file.as_ref())?;
        file_contents.push(content);
    }

    let mut interfaces = Vec::new();
    for content in &file_contents {
        let interface = Interface::try_from(content.as_str())?;
        interfaces.push(interface);
    }

    // Generate code for all interfaces
    let mut output = match generate_interfaces(&interfaces) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Failed to generate code for interfaces: {e}");
            return Err(Error::CodegenFailed);
        }
    };

    // Format the code if requested
    if config.rustfmt {
        output = match format_code(&output) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Failed to format code: {e}");
                return Err(Error::FormatFailed);
            }
        };
    }

    // Write output to file
    fs::write(output_file.as_ref(), output)?;

    Ok(true)
}
