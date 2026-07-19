use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Reply;
use pin_project_lite::pin_project;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

/// A notified state (e.g a field) of a service implementation.
#[derive(Debug, Clone)]
pub struct State<T, ReplyParams> {
    value: T,
    tx: broadcast::Sender<ReplyParams>,
}

impl<T, ReplyParams> zlink_core::notified::State<T, ReplyParams> for State<T, ReplyParams>
where
    T: Into<ReplyParams> + Clone + Debug + Send,
    ReplyParams: Clone + Send + 'static + Debug,
{
    type Stream = Stream<ReplyParams>;

    /// Create a new notified field.
    fn new(value: T) -> Self {
        let (tx, _) = broadcast::channel(1);

        Self { value, tx }
    }

    /// Set the value of the notified field and notify all listeners.
    async fn set(&mut self, value: T) {
        self.value = value.clone();
        // Failure means that there are currently no receivers and that's ok.
        let _ = self.tx.send(value.into());
    }

    /// Get the value of the notified field.
    fn get(&self) -> T {
        self.value.clone()
    }

    /// Get a stream of replies for the notified field.
    fn stream(&self) -> Stream<ReplyParams> {
        Stream {
            inner: self.tx.subscribe().into(),
            cached: None,
            once: false,
        }
    }

    /// A stream of replies for this state, that only yields one reply: the current state.
    fn stream_once(&self) -> Stream<ReplyParams> {
        Stream {
            inner: self.tx.subscribe().into(),
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
        inner: BroadcastStream<ReplyParams>,
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
        let mut stream = this.inner;
        loop {
            match futures_util::ready!(stream.as_mut().poll_next(cx)) {
                Some(Ok(reply)) => {
                    // Cache and yield immediately with continues=true.
                    *this.cached = Some(reply.clone());
                    break Poll::Ready(Some(Reply::new(Some(reply)).set_continues(Some(true))));
                }
                // Some intermediate values were missed. That's OK, as long as we get the
                // latest value.
                Some(Err(_)) => continue,
                // Channel closed - yield cached value with continues=false.
                None => {
                    break Poll::Ready(
                        this.cached
                            .take()
                            .map(|reply| Reply::new(Some(reply)).set_continues(Some(false))),
                    );
                }
            }
        }
    }
}
