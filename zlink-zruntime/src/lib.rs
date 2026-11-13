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

/// Runs a future to completion on the calling thread, together with zruntime.
///
/// This is for a program that has no async runtime to poll zlink's futures with: put the async
/// code in one call to it, and the call blocks the thread until the future completes.
///
/// The sockets that [`unix`] makes inside the call are on the runtime of the calling thread:
/// between polls of the future, the thread runs their I/O, so their traffic needs no other thread.
/// They work under any other executor as well: where no call to this function runs their I/O, a
/// helper thread of zruntime does. `connect_local` and `bind_local` instead require a running
/// `LocalRuntime`.
///
/// The future must not block the thread to wait for the sockets, for example by waiting
/// synchronously for a reply: their I/O runs on this same thread, between polls of the future, so
/// such a wait never ends.
///
/// # Panics
///
/// Panics if the calling thread is running zruntime already, as it is inside the future of another
/// call to this function: the inner call would wait for its own thread. Do not call it from a task
/// of another executor either: it blocks that executor's thread until the future completes, which
/// deadlocks if the future needs that thread.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    zruntime::block_on(future)
}
