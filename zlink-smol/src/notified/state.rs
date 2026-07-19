use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Reply;
use async_broadcast::{
    InactiveReceiver, Receiver as BroadcastReceiver, Sender as BroadcastSender, broadcast,
};
use pin_project_lite::pin_project;

/// A notified state (e.g a field) of a service implementation.
#[derive(Debug, Clone)]
pub struct State<T, ReplyParams> {
    value: T,
    tx: BroadcastSender<ReplyParams>,
    // Keep an inactive receiver to prevent the channel from closing.
    inactive_rx: InactiveReceiver<ReplyParams>,
}

impl<T, ReplyParams> zlink_core::notified::State<T, ReplyParams> for State<T, ReplyParams>
where
    T: Into<ReplyParams> + Clone + Debug + Send,
    ReplyParams: Clone + Send + 'static + Debug,
{
    type Stream = Stream<ReplyParams>;

    /// Create a new notified field.
    fn new(value: T) -> Self {
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
    async fn set(&mut self, value: T) {
        self.value = value.clone();
        self.tx
            .broadcast_direct(value.into())
            .await
            // Since we enabled overflow and disabled awaiting active receivers, this can't fail.
            .expect("Failed to broadcast value");
    }

    /// The value of the notified field.
    fn get(&self) -> T {
        self.value.clone()
    }

    /// A stream of replies for the notified field.
    fn stream(&self) -> Stream<ReplyParams> {
        Stream {
            inner: self.inactive_rx.activate_cloned(),
            cached: None,
            once: false,
        }
    }

    /// A stream of replies for this state, that only yields one reply: the current state.
    fn stream_once(&self) -> Stream<ReplyParams> {
        Stream {
            inner: self.inactive_rx.activate_cloned(),
            cached: Some(self.get().into()),
            once: true,
        }
    }
}

pin_project! {
    /// The stream to use as the [`zlink_core::Service::ReplyStream`] in service implementation when
    /// using [`State`].
    #[derive(Debug)]
    pub struct Stream<ReplyParams> {
        #[pin]
        inner: BroadcastReceiver<ReplyParams>,
        cached: Option<ReplyParams>,
        once: bool,
    }
}

impl<ReplyParams> futures_util::Stream for Stream<ReplyParams>
where
    ReplyParams: Clone + Send + 'static,
{
    type Item = Reply<ReplyParams>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if *this.once {
            return Poll::Ready(
                this.cached
                    .take()
                    .map(|reply| Reply::new(Some(reply)).set_continues(Some(false))),
            );
        }
        match futures_util::ready!(this.inner.poll_next(cx)) {
            Some(reply) => {
                // Cache and yield immediately with continues=true.
                *this.cached = Some(reply.clone());
                Poll::Ready(Some(Reply::new(Some(reply)).set_continues(Some(true))))
            }
            // Channel closed - yield cached value with continues=false.
            None => Poll::Ready(
                this.cached
                    .take()
                    .map(|reply| Reply::new(Some(reply)).set_continues(Some(false))),
            ),
        }
    }
}
