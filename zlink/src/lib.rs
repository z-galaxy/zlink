#![cfg_attr(not(any(feature = "tokio", feature = "smol")), no_std)]
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
#![doc = include_str!("../README.md")]

#[cfg(not(any(feature = "tokio", feature = "smol")))]
compile_error!("At least one runtime feature must be enabled: 'tokio' or 'smol'");

#[cfg(doctest)]
mod doctests {
    // Book markdown checks.
    doc_comment::doctest!("../../book/src/introduction.md");
    doc_comment::doctest!("../../book/src/concepts.md");
    doc_comment::doctest!("../../book/src/connection.md");
    #[cfg(feature = "proxy")]
    doc_comment::doctest!("../../book/src/client.md");
    #[cfg(feature = "service")]
    doc_comment::doctest!("../../book/src/service.md");
    #[cfg(feature = "proxy")]
    doc_comment::doctest!("../../book/src/pipelining.md");
    doc_comment::doctest!("../../book/src/design.md");
    // The introspection chapter is checked from `zlink-codegen` since its examples depend on that
    // crate, which we can't (dev-)depend on without creating a dependency cycle. Similarly, the
    // embedded chapter is checked from `zlink-core` since its example requires the `no_std`
    // variants of the socket traits.
    doc_comment::doctest!("../../book/src/faq.md");
}

#[cfg(feature = "tokio")]
pub use zlink_tokio::*;

#[cfg(all(feature = "smol", not(feature = "tokio")))]
pub use zlink_smol::*;
