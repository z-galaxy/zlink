#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
//! Code generation for Varlink interfaces.

use anyhow::{Context, Result};
use zlink::idl::Interface;

mod codegen;
pub use codegen::CodeGenerator;

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
