# Introspection & code generation

<!-- toc -->

We've seen that the [`service` macro](service.html#introspection-for-free) implements the standard
`org.varlink.service` introspection interface for you. This chapter covers the other side — how to
*consume* introspection from a client — and how to turn IDL into ready-made Rust code.

The relevant cargo features are:

* `idl` — the Rust representation of Varlink IDL (`zlink::idl` module: `Interface`, `Method`,
  `Type` and friends);
* `idl-parse` — parsing IDL text into those types at runtime (works in `no_std`, too);
* `introspection` — the `introspect` module: traits and derives that describe Rust types in IDL
  terms at compile time, plus the `org.varlink.service` client support.

## Querying a service

The `zlink::varlink_service` module ships a ready-made [proxy](client.html) for
`org.varlink.service`. Since proxy traits are implemented directly on `Connection`, *any*
connection can be introspected — just bring the trait into scope:

```rust,no_run
use zlink::varlink_service::Proxy as _;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = zlink::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    // Who are you?
    let info = conn.get_info().await?.map_err(|e| e.to_string())?;
    println!("{} {} (by {})", info.product, info.version, info.vendor);
    for interface in &info.interfaces {
        println!("  implements {interface}");
    }

    // Tell me about that interface.
    let description = conn
        .get_interface_description("io.systemd.Resolve")
        .await?
        .map_err(|e| e.to_string())?;
    let interface = description.parse()?; // -> zlink::idl::Interface (needs `idl-parse`)
    for method in interface.methods() {
        println!("method {}", method.name());
    }

    Ok(())
}
```

The parsed `Interface` gives you programmatic access to everything in the IDL: methods with their
parameters, custom types (`interface.custom_types()`), errors (`interface.errors()`), and doc
comments. The repository's [`varlink-inspect`] example is a tiny `varlinkctl introspect` clone
built on exactly this API:

```bash
cargo run --example varlink-inspect --features="introspection idl-parse" -- \
  /run/systemd/resolve/io.systemd.Resolve
```

## Describing your types: the `introspect` derives

For the compile-time direction — describing Rust types in IDL terms so the `service` macro can
generate interface descriptions — the `introspect` module provides three derives:

* `introspect::Type` for structs and enums used as parameters or replies;
* `introspect::CustomType` for types that should appear as *named* custom types in the IDL;
* `introspect::ReplyError` for error enums (derived alongside `zlink::ReplyError`).

You've already seen them in earlier chapters; they all support `#[zlink(rename)]`/`rename_all` to
control the IDL-level naming.

## Code generation

Writing proxy traits and types by hand is fine for small interfaces, but when a service publishes
its IDL, you can generate the Rust side mechanically with **`zlink-codegen`** — zlink's analog of
zbus's `zbus_xmlgen`:

```bash
# Install the code generator
cargo install zlink-codegen

# Get the IDL of a running service...
varlinkctl introspect /run/systemd/resolve/io.systemd.Resolve io.systemd.Resolve \
  > io.systemd.Resolve.varlink

# ...and generate Rust code from it
zlink-codegen io.systemd.Resolve.varlink > src/resolve.rs
```

For each interface, the generated code contains a `#[proxy]` trait with one method per IDL method,
structs for the custom types (and for multi-value method returns), and a `ReplyError` enum covering
the interface's errors. IDL types map the way you'd expect: `?T` → `Option<T>`, `[]T` → `Vec<T>`,
`[string]T` → `HashMap<String, T>`; input parameters use borrowed types (`&str`, `&[T]`) for
zero-copy calls.

### From a build script

`zlink-codegen` is also a library, so you can regenerate bindings on every build instead of
committing them:

```rust,no_run
// build.rs
use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let idl = PathBuf::from(&manifest_dir).join("io.systemd.Resolve.varlink");
    println!("cargo:rerun-if-changed={}", idl.display());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("generated.rs");
    zlink_codegen::generate_files(&zlink_codegen::CodegenOptions {
        files: vec![idl],
        output: Some(out),
        rustfmt: true,
        ..Default::default()
    })
    .expect("Failed to generate code");
}
```

...and then pull the generated code into your crate with
`include!(concat!(env!("OUT_DIR"), "/generated.rs"));` — the
[`test-integration` crate][test-integration] in the zlink repository demonstrates this exact setup
end to end.

As with `zbus_xmlgen`, treat generator output as a starting point when hand-tuning is warranted —
e.g. to use richer Rust types than the IDL can express — and as a fully-automatic solution when it
isn't.

[`varlink-inspect`]: https://github.com/z-galaxy/zlink/blob/main/zlink/examples/varlink-inspect.rs
[test-integration]: https://github.com/z-galaxy/zlink/tree/main/zlink-codegen/test-integration
