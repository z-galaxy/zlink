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

pub(crate) use zlink_core::*;
pub mod byte_stream;
#[cfg(feature = "server")]
pub mod notified;
pub mod unix;

/// Runs a future to completion on the calling thread, driving the I/O of smol with it.
///
/// This is for a program that has no async runtime to poll zlink's futures with: put the async
/// code in one call to it, and the call blocks the thread until the future completes. It is the
/// `block_on` of smol and async-io: while the future waits, the calling thread runs the I/O of the
/// sockets of [`unix`], whose reactor the whole process shares. Those sockets work under any other
/// executor as well: where no call to this function runs their I/O, a thread of async-io does.
///
/// Do not call it from a task of another executor: it blocks that executor's thread until the
/// future completes, which deadlocks if the future needs that thread.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    async_io::block_on(future)
}
