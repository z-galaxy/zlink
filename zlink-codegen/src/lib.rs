#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
//! Code generation for Varlink interfaces.

use std::{fs, path::PathBuf};
use zlink::idl::Interface;

#[cfg(doctest)]
mod doctests {
    // Book markdown checks. The other chapters are checked from the `zlink` crate; this one is
    // checked from here because its code-generation examples depend on this crate, which `zlink`
    // can't (dev-)depend on without creating a dependency cycle.
    doc_comment::doctest!("../../book/src/introspection.md");
}

mod codegen;
pub use codegen::CodeGenerator;
mod error;
pub use self::error::Error;

/// Generate Rust code from a Varlink interface.
///
/// # Errors
///
/// - [`Error::Fmt`] - Writing to the internal output buffer failed.
pub fn generate_interface(interface: &Interface<'_>) -> Result<String, Error> {
    let mut generator = CodeGenerator::new();
    generator.generate_interface(interface, false)?;
    Ok(generator.output())
}

/// Generate Rust code from multiple Varlink interfaces.
///
/// # Errors
///
/// - [`Error::Fmt`] - Writing to the internal output buffer failed.
pub fn generate_interfaces(interfaces: &[Interface<'_>]) -> Result<String, Error> {
    let mut generator = CodeGenerator::new();

    // Add module-level header for multiple interfaces.
    if interfaces.len() > 1 {
        generator.write_module_header()?;
    }

    for interface in interfaces {
        // Skip module header for all interfaces when generating multiple.
        let skip_header = interfaces.len() > 1;
        generator.generate_interface(interface, skip_header)?;
    }
    Ok(generator.output())
}

/// Format generated Rust code using rustfmt.
///
/// # Errors
///
/// - [`Error::Io`] - Spawning or communicating with the `rustfmt` process failed.
/// - [`Error::InvalidUtf8`] - `rustfmt` produced output that was not valid UTF-8.
pub fn format_code(code: &str) -> Result<String, Error> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let mut child = Command::new("rustfmt")
        .arg("--edition=2021")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(code.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        // If rustfmt fails, return the original code.
        eprintln!(
            "Warning: rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        return Ok(code.to_string());
    }

    String::from_utf8(output.stdout).map_err(Error::from)
}

/// Configuration options for Varlink code generation.
///
/// See [`generate_files`] for an end-to-end usage example.
#[derive(Default)]
pub struct CodegenOptions {
    /// Input Varlink IDL file(s).
    pub files: Vec<PathBuf>,
    /// Output file path (defaults to stdout if not specified).
    pub output: Option<PathBuf>,
    /// Generate separate files for each interface (ignored if output is specified).
    pub multiple_files: bool,
    /// Whether to format the generated Rust code using `rustfmt`.
    ///
    /// Default value: false
    pub rustfmt: bool,
}

/// Generate Rust source files from Varlink interface files.
///
/// This function reads Varlink interface definition files from `config.files`, parses them,
/// generates corresponding Rust code, and handles output based on the configuration.
///
/// # Behavior
///
/// - If `config.output` is specified: Generates a single file with all interfaces combined.
/// - If `config.multiple_files` is true: Generates separate `.rs` files for each interface in the
///   current working directory. The output filename is derived from the interface name (dots
///   replaced with underscores and converted to lowercase).
/// - Otherwise: Writes the generated code to stdout.
///
/// # Arguments
///
/// * `config` - Code generation options containing input files, output configuration, and
///   formatting options.
///
/// # Returns
///
/// Returns `Ok(())` on successful code generation and output.
///
/// # Errors
///
/// This function will return an error if:
/// - [`Error::InvalidArgument`] - No input files were specified.
/// - [`Error::Io`] - File I/O failed (reading input, writing output or invoking `rustfmt`).
/// - [`Error::Zlink`] - A Varlink interface definition is malformed or invalid.
/// - [`Error::Fmt`] - Writing to the internal output buffer failed.
/// - [`Error::InvalidUtf8`] - `rustfmt` produced output that was not valid UTF-8.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// use zlink_codegen::CodegenOptions;
///
/// let config = CodegenOptions {
///     files: vec![PathBuf::from("interface.varlink")],
///     output: Some(PathBuf::from("generated.rs")),
///     rustfmt: true,
///     ..Default::default()
/// };
/// zlink_codegen::generate_files(&config).expect("Failed to generate code");
/// ```
pub fn generate_files(config: &CodegenOptions) -> Result<(), Error> {
    use std::io::Write;

    if config.files.is_empty() {
        return Err(Error::InvalidArgument);
    }

    // Read and parse all interface files
    let mut file_contents = Vec::new();
    for interface_file in &config.files {
        let content = fs::read_to_string(interface_file)?;
        file_contents.push(content);
    }

    let mut interfaces = Vec::new();
    for content in &file_contents {
        let interface = Interface::try_from(content.as_str()).map_err(zlink::Error::from)?;
        interfaces.push(interface);
    }

    // Determine output mode
    if let Some(output_path) = &config.output {
        // Single output file mode
        let code = if interfaces.len() == 1 {
            generate_interface(&interfaces[0])?
        } else {
            generate_interfaces(&interfaces)?
        };

        let output = if config.rustfmt {
            format_code(&code)?
        } else {
            code
        };

        fs::write(output_path, output)?;
    } else if config.multiple_files {
        // Multiple output files mode - generate separate file for each interface
        for interface in &interfaces {
            let code = generate_interface(interface)?;

            let output = if config.rustfmt {
                format_code(&code)?
            } else {
                code
            };

            // Generate output filename from interface name
            let filename = interface_to_filename(interface.name());
            let output_path = PathBuf::from(filename);

            fs::write(&output_path, output)?;
        }
    } else {
        // Stdout mode
        let code = if interfaces.len() == 1 {
            generate_interface(&interfaces[0])?
        } else {
            generate_interfaces(&interfaces)?
        };

        let output = if config.rustfmt {
            format_code(&code)?
        } else {
            code
        };

        std::io::stdout().write_all(output.as_bytes())?;
    }

    Ok(())
}

/// Convert an interface name to a filename.
///
/// The filename is converted to lowercase to comply with Rust's naming conventions.
///
/// For example: `"org.example.Interface"` → `"org_example_interface.rs"`
fn interface_to_filename(interface_name: &str) -> String {
    format!("{}.rs", interface_name.replace('.', "_").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_to_filename() {
        assert_eq!(
            interface_to_filename("org.example.Interface"),
            "org_example_interface.rs"
        );
        assert_eq!(
            interface_to_filename("com.example.MyService"),
            "com_example_myservice.rs"
        );
        assert_eq!(
            interface_to_filename("SimpleInterface"),
            "simpleinterface.rs"
        );
        assert_eq!(
            interface_to_filename("org.varlink.service"),
            "org_varlink_service.rs"
        );
    }
}
