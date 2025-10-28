//! Convenience API for maintaining state, that notifies on changes.

use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Reply;
use async_broadcast::{
    broadcast, InactiveReceiver, Receiver as BroadcastReceiver, Sender as BroadcastSender,
};
use async_channel::{bounded, Receiver as OneshotReceiver, Sender as OneshotSender};
use pin_project_lite::pin_project;

/// A notified state (e.g a field) of a service implementation.
#[derive(Debug, Clone)]
pub struct State<T, ReplyParams> {
    value: T,
    tx: BroadcastSender<ReplyParams>,
    // Keep an inactive receiver to prevent the channel from closing.
    inactive_rx: InactiveReceiver<ReplyParams>,
}

impl<T, ReplyParams> State<T, ReplyParams>
where
    T: Into<ReplyParams> + Clone + Debug,
    ReplyParams: Clone + Send + 'static + Debug,
{
    /// Create a new notified field.
    pub fn new(value: T) -> Self {
        let (mut tx, rx) = broadcast(1);
        // Notification broadcast shouldn't await active subscribers.
        tx.set_await_active(false);
        // Enable overflow mode because:
        // 1. We don't need to ensure that subscribers receive all values, as long as they always
        //    receive the latest value so we don't want the broadcast to wait for receivers.
        // 2. This would be consistent with the behavior of the `zlink_tokio::notified::State`.
        tx.set_overflow(true);
        // Deactivate the initial receiver to keep the channel open without consuming buffer space.
        let inactive_rx = rx.deactivate();

        Self {
            value,
            tx,
            inactive_rx,
        }
    }

    /// Set the value of the notified field and notify all listeners.
    pub async fn set(&mut self, value: T) {
        self.value = value.clone();
        self.tx
            .broadcast_direct(value.into())
            .await
            // Since we enabled overflow and disabled awaiting active receivers, this can't fail.
            .expect("Failed to broadcast value");
    }

    /// The value of the notified field.
    pub fn get(&self) -> T {
        self.value.clone()
    }

    /// A stream of replies for the notified field.
    pub fn stream(&self) -> Stream<ReplyParams> {
        Stream(Box::pin(StreamInner::Broadcast {
            receiver: self.inactive_rx.activate_cloned(),
        }))
    }
}

/// A one-shot notified state of a service implementation.
///
/// This is useful for handling method calls in a separate task/thread.
#[derive(Debug)]
pub struct Once<ReplyParams> {
    tx: OneshotSender<ReplyParams>,
}

impl<ReplyParams> Once<ReplyParams>
where
    ReplyParams: Send + 'static + Debug,
{
    /// Create a new notified oneshot state.
    pub fn new() -> (Self, Stream<ReplyParams>) {
        let (tx, rx) = bounded(1);

        (
            Self { tx },
            Stream(Box::pin(StreamInner::Oneshot {
                receiver: rx,
                terminated: false,
            })),
        )
    }

    /// Set the value of the notified field and notify all listeners.
    pub fn notify<T>(self, value: T)
    where
        T: Into<ReplyParams> + Debug,
    {
        // Failure means that we dropped the receiver stream internally before it received anything
        // and that's a big bug that must not happen.
        self.tx.try_send(value.into()).unwrap();
    }
}

/// The stream to use as the [`crate::Service::ReplyStream`] in service implementation when using
/// [`State`] or [`Once`].
pub struct Stream<ReplyParams>(Pin<Box<StreamInner<ReplyParams>>>);

impl<ReplyParams> std::fmt::Debug for Stream<ReplyParams>
where
    ReplyParams: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Stream").field(&self.0).finish()
    }
}

impl<ReplyParams> futures_util::Stream for Stream<ReplyParams>
where
    ReplyParams: Clone + Send + 'static,
{
    type Item = Reply<ReplyParams>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Since Stream is Unpin, we can get mutable access
        let this = self.get_mut();
        // Project through the Pin<Box<StreamInner>> to the receivers
        match this.0.as_mut().project() {
            StreamInnerProj::Broadcast { receiver } => {
                match futures_util::ready!(receiver.poll_next(cx)) {
                    Some(reply) => {
                        Poll::Ready(Some(Reply::new(Some(reply)).set_continues(Some(true))))
                    }
                    None => Poll::Ready(None),
                }
            }
            StreamInnerProj::Oneshot {
                receiver,
                terminated,
            } => {
                if *terminated {
                    return Poll::Ready(None);
                }

                match futures_util::ready!(receiver.poll_next(cx)) {
                    Some(reply) => {
                        *terminated = true;
                        Poll::Ready(Some(Reply::new(Some(reply)).set_continues(Some(false))))
                    }
                    None => Poll::Ready(None),
                }
            }
        }
    }
}

pin_project! {
    #[project = StreamInnerProj]
    #[derive(Debug)]
    enum StreamInner<ReplyParams> {
        Broadcast {
            #[pin]
            receiver: BroadcastReceiver<ReplyParams>,
        },
        Oneshot {
            #[pin]
            receiver: OneshotReceiver<ReplyParams>,
            terminated: bool,
        },
    }
}
