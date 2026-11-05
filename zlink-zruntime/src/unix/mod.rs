//! Provides transport over Unix Domain Sockets.
//!
//! The sockets are on a zruntime runtime of either flavour: [`connect`] and [`bind`] put them on
//! the shared runtime [`SharedRuntime::current`] returns, whose sockets can be used from any thread
//! and polled by any executor, and [`connect_local`] and [`bind_local`] on the local runtime the
//! calling thread is running, whose sockets stay on that thread.

use std::io;

use zruntime::LocalRuntime;
#[cfg(doc)]
use zruntime::SharedRuntime;

mod stream;
pub use stream::{Connection, ReadHalf, Stream, WriteHalf, connect, connect_local};
#[cfg(feature = "server")]
mod listener;
#[cfg(feature = "server")]
pub use listener::{Listener, bind, bind_local};

/// The local runtime the calling thread is running, for the sockets made on it.
fn running_local() -> io::Result<LocalRuntime> {
    LocalRuntime::running().ok_or_else(|| {
        io::Error::other(
            "no local runtime is running on this thread: connect or bind from inside \
             `LocalRuntime::block_on`, or from a task of that runtime",
        )
    })
}
