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
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

pub(crate) use zlink_core::*;
#[cfg(feature = "server")]
pub mod notified;
pub mod unix;

/// Runs a future to completion on the calling thread, on a Tokio runtime of zlink's own.
///
/// This is for a program that has no Tokio runtime to poll zlink's futures with: put the async
/// code in one call to it, and the call blocks the thread until the future completes. The sockets
/// of [`unix`] need a Tokio runtime, and this function provides one: a single-threaded runtime,
/// made on first use and kept for the rest of the process, so that a socket made inside one call
/// keeps working in the next ones. It runs the I/O of its sockets only while a thread is inside
/// this function, on one such thread at a time, so a socket makes no progress outside of a call.
///
/// # Panics
///
/// Panics if called inside a Tokio runtime or if runtime initialization fails.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    RUNTIME
        .get_or_init(|| {
            Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to make the Tokio runtime of `block_on`")
        })
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixStream,
    };

    /// The runtime outlives the call, so a socket made inside one call works in the next.
    #[test]
    fn a_socket_works_in_the_next_call() {
        let (mut a, mut b) = super::block_on(async { UnixStream::pair() }).unwrap();

        let mut byte = [0];
        super::block_on(async {
            a.write_all(b"!").await.unwrap();
            b.read_exact(&mut byte).await.unwrap();
        });

        assert_eq!(&byte, b"!");
    }
}
