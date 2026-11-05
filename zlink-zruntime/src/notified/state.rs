use std::{
    fmt::Debug,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll, ready},
};

use crate::Reply;
use pin_project_lite::pin_project;
use zruntime::broadcast::{
    InactiveReceiver, Receiver as BroadcastReceiver, Sender as BroadcastSender, channel,
};

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
        let (mut tx, rx) = channel(NonZeroUsize::MIN);
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
        // In overflow mode, the channel is never full. The inactive receiver keeps it open, so the
        // only failure is that there are currently no active receivers and that's ok.
        let _ = self.tx.try_broadcast(value.into());
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

impl<ReplyParams> futures_core::Stream for Stream<ReplyParams>
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
        // Missed values are skipped. That's OK, as long as we get the latest value.
        match ready!(this.inner.poll_next(cx)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use zlink_core::notified::State as _;

    #[test]
    fn set_without_streams() {
        zruntime::block_on(async {
            let mut state = State::<u32, u32>::new(1);
            state.set(2).await;
            assert_eq!(state.get(), 2);

            // A stream only sees what is set once it exists.
            let mut stream = state.stream();
            state.set(3).await;
            let reply = std::future::poll_fn(|cx| {
                futures_core::Stream::poll_next(Pin::new(&mut stream), cx)
            })
            .await
            .unwrap();
            assert_eq!(reply.continues(), Some(true));
            assert_eq!(reply.into_parameters(), Some(3));
        });
    }
}
