//! CLI tool for inspecting Varlink services.
//!
//! This example demonstrates how to use the `varlink_service::Proxy` trait to
//! introspect Varlink services by connecting to Unix domain sockets.

use clap::Parser;
use colored::*;
use std::process;
use zlink::{tokio::unix, varlink_service::Proxy};

#[derive(Parser)]
#[command(name = "varlink-inspect")]
#[command(about = "Inspect Varlink services via Unix domain sockets")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Path to the Unix domain socket
    #[arg(help = "Unix socket path (e.g., /run/systemd/machine/io.systemd.Machine)")]
    socket_path: String,

    /// Optional interface name to inspect
    #[arg(help = "Interface name to get detailed description (e.g., io.systemd.Machine)")]
    interface: Option<String>,
}

#[tokio::main]
async fn main() {
    // Setup tracing subscriber
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    if let Err(e) = run(&cli).await {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

async fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to socket: {}", cli.socket_path);
    println!();

    // Connect to the Unix domain socket.
    let mut conn = unix::connect(&cli.socket_path).await?;

    // Get service information.
    let info_result = conn.get_info().await?;
    let info = match info_result {
        Ok(info) => info,
        Err(error) => {
            return Err(format!("Failed to get service info: {error:?}").into());
        }
    };

    // Display service information nicely.
    print_service_info(&info);

    // If an interface name was provided, get and display interface details.
    if let Some(interface_name) = &cli.interface {
        println!();
        println!("{}", "─".repeat(60));
        println!();

        let desc_result = conn.get_interface_description(interface_name).await?;
        let description = match desc_result {
            Ok(desc) => desc,
            Err(error) => {
                return Err(format!("Failed to get interface description: {error:?}").into());
            }
        };

        let interface = description.parse()?;
        print_interface_details(&interface);
    }

    Ok(())
}

fn print_service_info(info: &zlink::varlink_service::Info<'_>) {
    println!("{}", "🔍 Service Information".cyan().bold());
    println!("{}", "━".repeat(60).cyan());
    println!("  {}     {}", "Vendor:".bold(), info.vendor);
    println!("  {}    {}", "Product:".bold(), info.product);
    println!("  {}    {}", "Version:".bold(), info.version);
    println!("  {}        {}", "URL:".bold(), info.url.blue());
    println!();
    println!(
        "{} ({}):",
        "📋 Available Interfaces".cyan().bold(),
        info.interfaces.len()
    );
    for (i, interface) in info.interfaces.iter().enumerate() {
        println!("  {}. {}", i + 1, interface.bright_blue());
    }
}

fn print_interface_details(interface: &zlink::idl::Interface<'_>) {
    println!(
        "{} {}",
        "🔧 Interface:".cyan().bold(),
        interface.name().bright_blue().bold()
    );
    println!("{}", "━".repeat(60).cyan());

    // Print interface comments if any.
    let has_interface_comments = print_comments(interface.comments(), "");
    if has_interface_comments {
        println!();
    }

    // Print methods.
    let methods: Vec<_> = interface.methods().collect();
    if !methods.is_empty() {
        println!();
        println!("{} ({}):", "Methods".cyan().bold(), methods.len());
        for method in methods.iter() {
            println!("  📞 {}", method.name().green().bold());

            // Print method comments.
            print_comments(method.comments(), "     ");

            // Print input parameters.
            let inputs: Vec<_> = method.inputs().collect();
            for input in inputs {
                println!("     ➡️ {}: {}", input.name(), format_type(input.ty()));
                print_comments(input.comments(), "       ");
            }

            // Print output parameters.
            let outputs: Vec<_> = method.outputs().collect();
            for output in outputs {
                println!("     ⬅️ {}: {}", output.name(), format_type(output.ty()));
                print_comments(output.comments(), "       ");
            }
        }
    }

    // Print custom types.
    let custom_types: Vec<_> = interface.custom_types().collect();
    if !custom_types.is_empty() {
        println!();
        println!("{} ({}):", "Custom Types".cyan().bold(), custom_types.len());
        for custom_type in custom_types.iter() {
            let kind = match custom_type {
                zlink::idl::CustomType::Object(_) => "object",
                zlink::idl::CustomType::Enum(_) => "enum",
            };
            println!(
                "  🏗️ {} ({})",
                custom_type.name().magenta().bold(),
                kind.dimmed()
            );

            // Print custom type comments.
            match custom_type {
                zlink::idl::CustomType::Object(obj) => {
                    print_comments(obj.comments(), "     ");
                }
                zlink::idl::CustomType::Enum(enum_type) => {
                    print_comments(enum_type.comments(), "     ");
                }
            }

            match custom_type {
                zlink::idl::CustomType::Object(obj) => {
                    let fields: Vec<_> = obj.fields().collect();
                    if !fields.is_empty() {
                        println!("     {}:", "Fields".bold());
                        for field in fields {
                            println!("       • {}: {}", field.name(), format_type(field.ty()));
                            print_comments(field.comments(), "         ");
                        }
                    }
                }
                zlink::idl::CustomType::Enum(enum_type) => {
                    let variants: Vec<_> = enum_type.variants().collect();
                    if !variants.is_empty() {
                        println!("     {}:", "Variants".bold());
                        for variant in variants {
                            println!("       • {}", variant);
                        }
                    }
                }
            }
        }
    }

    // Print errors.
    let errors: Vec<_> = interface.errors().collect();
    if !errors.is_empty() {
        println!();
        println!("{} ({}):", "Errors".cyan().bold(), errors.len());
        for error in errors.iter() {
            println!("  ⚠️ {}", error.name().yellow().bold());

            // Print error comments.
            print_comments(error.comments(), "     ");

            let fields: Vec<_> = error.fields().collect();
            if !fields.is_empty() {
                println!("     {}:", "Fields".bold());
                for field in fields {
                    println!("       • {}: {}", field.name(), format_type(field.ty()));
                    print_comments(field.comments(), "         ");
                }
            }
        }
    }

    // Print interface statistics.
    println!();
    println!("{}", "📊 Summary:".cyan().bold());
    println!("  • {} methods", methods.len().to_string().yellow().bold());
    println!(
        "  • {} custom types",
        custom_types.len().to_string().magenta().bold()
    );
    println!("  • {} error types", errors.len().to_string().red().bold());
}

fn print_comments<'a>(
    comments: impl Iterator<Item = &'a zlink::idl::Comment<'a>>,
    indent: &str,
) -> bool {
    let mut printed_any = false;
    for comment in comments {
        println!("{}📝 {}", indent, comment.text().truecolor(156, 163, 175));
        printed_any = true;
    }
    printed_any
}

fn format_type(ty: &zlink::idl::Type<'_>) -> ColoredString {
    match ty {
        zlink::idl::Type::Bool => "bool".bright_blue(),
        zlink::idl::Type::Int => "int".bright_blue(),
        zlink::idl::Type::Float => "float".bright_blue(),
        zlink::idl::Type::String => "string".bright_blue(),
        zlink::idl::Type::ForeignObject => "object".bright_blue(),
        zlink::idl::Type::Any => "any".bright_blue(),
        zlink::idl::Type::Array(type_ref) => format!("[{}]", format_type(type_ref)).bright_blue(),
        zlink::idl::Type::Optional(type_ref) => format!("?{}", format_type(type_ref)).bright_blue(),
        zlink::idl::Type::Map(type_ref) => {
            format!("[string]{}", format_type(type_ref)).bright_blue()
        }
        zlink::idl::Type::Object(_) => "object".bright_blue(),
        zlink::idl::Type::Custom(name) => name.magenta(),
        zlink::idl::Type::Enum(variants) => {
            let variant_names: Vec<&str> = variants.iter().map(|v| v.name()).collect();
            format!("({})", variant_names.join(", ")).bright_blue()
        }
    }
}
