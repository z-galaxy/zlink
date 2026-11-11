#![cfg_attr(
    not(any(feature = "tokio", feature = "smol", feature = "zruntime")),
    no_std
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs
)]
#![warn(unreachable_pub)]
// The README examples use the zruntime runtime and hence the `zruntime`-specific API.
#![cfg_attr(feature = "zruntime", doc = include_str!("../README.md"))]
#![cfg_attr(
    not(feature = "zruntime"),
    doc = "An asynchronous [Varlink](https://varlink.org/) API. See the \
        [project README](https://github.com/z-galaxy/zlink) for an overview and examples."
)]

#[cfg(not(any(feature = "tokio", feature = "smol", feature = "zruntime")))]
compile_error!("At least one runtime feature must be enabled: 'tokio', 'smol' or 'zruntime'");

// The book examples use the zruntime runtime and hence the `zruntime`-specific API.
#[cfg(all(doctest, feature = "zruntime"))]
mod doctests {
    // Book markdown checks.
    doc_comment::doctest!("../../book/src/introduction.md");
    doc_comment::doctest!("../../book/src/concepts.md");
    doc_comment::doctest!("../../book/src/connection.md");
    #[cfg(feature = "proxy")]
    doc_comment::doctest!("../../book/src/client.md");
    #[cfg(feature = "service")]
    doc_comment::doctest!("../../book/src/service.md");
    // Its example of a restart hands over through zoter, which is for Linux alone.
    #[cfg(all(feature = "service", feature = "handover", target_os = "linux"))]
    doc_comment::doctest!("../../book/src/restarts.md");
    #[cfg(feature = "proxy")]
    doc_comment::doctest!("../../book/src/pipelining.md");
    doc_comment::doctest!("../../book/src/design.md");
    // The introspection chapter is checked from `zlink-codegen` since its examples depend on that
    // crate, which we can't (dev-)depend on without creating a dependency cycle. Similarly, the
    // embedded chapter is checked from `zlink-core` since its example requires the `no_std`
    // variants of the socket traits.
    doc_comment::doctest!("../../book/src/faq.md");
}

pub use zlink_core::*;

/// API specific to the [tokio](https://tokio.rs/) runtime.
#[cfg(feature = "tokio")]
pub mod tokio {
    #[cfg(feature = "server")]
    pub use zlink_tokio::notified;
    pub use zlink_tokio::{block_on, unix};
}

/// API specific to the [smol](https://github.com/smol-rs/smol) runtime.
#[cfg(feature = "smol")]
pub mod smol {
    #[cfg(feature = "server")]
    pub use zlink_smol::notified;
    pub use zlink_smol::{block_on, unix};
}

/// API specific to the [zruntime](https://github.com/z-galaxy/zruntime) runtime.
#[cfg(feature = "zruntime")]
pub mod zruntime {
    #[cfg(feature = "server")]
    pub use zlink_zruntime::notified;
    pub use zlink_zruntime::{block_on, unix};
}
