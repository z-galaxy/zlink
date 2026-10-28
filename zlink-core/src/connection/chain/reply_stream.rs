use alloc::boxed::Box;
use core::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
};
use futures_util::stream::{Stream, unfold};
use serde::de::DeserializeOwned;

use crate::{
    Error, Result,
    connection::{ReadConnection, socket::ReadHalf},
    reply,
};

#[cfg(feature = "std")]
use std::os::fd::OwnedFd;

/// Type alias for chain reply results.
///
/// In std mode, includes file descriptors received with the reply.
/// In no_std mode, just the reply result.
#[cfg(feature = "std")]
pub(crate) type ChainResult<Params, ReplyError> =
    (reply::Result<Params, ReplyError>, alloc::vec::Vec<OwnedFd>);

#[cfg(not(feature = "std"))]
pub(crate) type ChainResult<Params, ReplyError> = reply::Result<Params, ReplyError>;

/// A stream of replies from a chain of method calls.
///
/// # Owned Data Requirement
///
/// Stream items must use owned types (`DeserializeOwned`) rather than borrowed types. This is
/// because the internal buffer may be reused between stream iterations, which would invalidate
/// borrowed references. This limitation may be lifted in the future when Rust supports lending
/// streams.
///
/// This is used internally by the proxy macro for streaming methods.
pub struct ReplyStream<'c, Params, ReplyError> {
    inner: InnerStream<'c, Params, ReplyError>,
}

impl<'c, Params, ReplyError> ReplyStream<'c, Params, ReplyError>
where
    Params: DeserializeOwned + Debug,
    ReplyError: DeserializeOwned + Debug,
{
    /// Create a new reply stream.
    ///
    /// The stream will yield `reply_count` replies from the connection.
    pub fn new<Read>(connection: &'c mut ReadConnection<Read>, reply_count: usize) -> Self
    where
        Read: ReadHalf + 'c,
    {
        // State is (connection, current_index). The connection reference flows through each
        // iteration.
        let inner = unfold(
            (connection, 0),
            move |(conn, mut current_index)| async move {
                if current_index >= reply_count {
                    return None;
                }

                let item = conn.receive_reply::<Params, ReplyError>().await;
                let item_ref = item.as_ref();
                #[cfg(feature = "std")]
                // In std mode, we need to ignore the FDs.
                let item_ref = item_ref.map(|r| &r.0);

                // Update index based on result.
                match item_ref {
                    Ok(Ok(r)) if r.continues() != Some(true) => {
                        current_index += 1;
                    }
                    Ok(Ok(_)) => {
                        // Streaming reply, don't increment index yet.
                    }
                    Ok(Err(_)) | Err(Error::VarlinkService(_)) => {
                        // For error replies, including those of the `org.varlink.service`
                        // interface, always increment since there won't be more replies to the
                        // call.
                        current_index += 1;
                    }
                    Err(_) => {
                        // General error, mark stream as done.
                        current_index = reply_count;
                    }
                }

                Some((item, (conn, current_index)))
            },
        );

        Self {
            inner: Box::pin(inner),
        }
    }
}

impl<Params, ReplyError> Debug for ReplyStream<'_, Params, ReplyError> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReplyStream").finish_non_exhaustive()
    }
}

impl<Params, ReplyError> Stream for ReplyStream<'_, Params, ReplyError> {
    type Item = Result<ChainResult<Params, ReplyError>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// The boxed inner stream type for `ReplyStream`.
type InnerStream<'c, Params, ReplyError> =
    Pin<Box<dyn Stream<Item = Result<ChainResult<Params, ReplyError>>> + 'c>>;
