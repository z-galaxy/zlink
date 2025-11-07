use std::{env, path::PathBuf};

fn main() {
    // Get the manifest directory.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // Process all IDL files.
    let idl_files = [
        "test.idl",
        "calc.idl",
        "storage.idl",
        "camelcase.idl",
        "anytype.idl",
    ];

    // Build paths to IDL files
    let idl_paths: Vec<PathBuf> = idl_files
        .iter()
        .map(|idl_file| PathBuf::from(&manifest_dir).join(idl_file))
        .collect();

    // Tell cargo to rerun if any IDL file changes.
    for idl_path in &idl_paths {
        println!("cargo:rerun-if-changed={}", idl_path.display());
    }

    // Write generated code to OUT_DIR.
    let out_dir = env::var("OUT_DIR").unwrap();
    let output_file = PathBuf::from(out_dir).join("generated.rs");

    // Generate code from all interface files
    let idl_path_refs: Vec<&PathBuf> = idl_paths.iter().collect();
    zlink_codegen::generate_files(
        &idl_path_refs,
        &output_file,
        &zlink_codegen::CodegenOptions {
            rustfmt: true,
            ..Default::default()
        },
    )
    .expect("Failed to generate code");
}
