//! Convenience API for maintaining state, that notifies on changes.

use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

use crate::Reply;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::BroadcastStream;

/// A notified state (e.g a field) of a service implementation.
#[derive(Debug, Clone)]
pub struct State<T, ReplyParams> {
    value: T,
    tx: broadcast::Sender<ReplyParams>,
}

impl<T, ReplyParams> State<T, ReplyParams>
where
    T: Into<ReplyParams> + Clone + Debug,
    ReplyParams: Clone + Send + 'static + Debug,
{
    /// Create a new notified field.
    pub fn new(value: T) -> Self {
        let (tx, _) = broadcast::channel(1);

        Self { value, tx }
    }

    /// Set the value of the notified field and notify all listeners.
    pub async fn set(&mut self, value: T) {
        self.value = value.clone();
        // Failure means that there are currently no receivers and that's ok.
        let _ = self.tx.send(value.into());
    }

    /// Get the value of the notified field.
    pub fn get(&self) -> T {
        self.value.clone()
    }

    /// Get a stream of replies for the notified field.
    pub fn stream(&self) -> Stream<ReplyParams> {
        Stream(StreamInner::Broadcast(self.tx.subscribe().into()))
    }
}

/// A one-shot notified state of a service implementation.
///
/// This is useful for handling method calls in a separate task/thread.
#[derive(Debug)]
pub struct Once<ReplyParams> {
    tx: oneshot::Sender<ReplyParams>,
}

impl<ReplyParams> Once<ReplyParams>
where
    ReplyParams: Send + 'static + Debug,
{
    /// Create a new notified oneshot state.
    pub fn new() -> (Self, Stream<ReplyParams>) {
        let (tx, rx) = oneshot::channel();

        (Self { tx }, Stream(StreamInner::Oneshot(rx)))
    }

    /// Set the value of the notified field and notify all listeners.
    pub fn notify<T>(self, value: T)
    where
        T: Into<ReplyParams> + Debug,
    {
        // Failure means that we dropped the receiver stream internally before it received anything
        // and that's a big bug that must not happen.
        self.tx.send(value.into()).unwrap();
    }
}

/// The stream to use as the [`crate::Service::ReplyStream`] in service implementation when using
/// [`State`] or [`Once`].
#[derive(Debug)]
pub struct Stream<ReplyParams>(StreamInner<ReplyParams>);

impl<ReplyParams> futures_util::Stream for Stream<ReplyParams>
where
    ReplyParams: Clone + Send + 'static,
{
    type Item = Reply<ReplyParams>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut self.0 {
            StreamInner::Broadcast(stream) => {
                let reply = loop {
                    match ready!(Pin::new(&mut *stream).poll_next(cx)) {
                        Some(Ok(reply)) => {
                            break Some(Reply::new(Some(reply)).set_continues(Some(true)));
                        }
                        // Some intermediate values were missed. That's OK, as long as we get the
                        // latest value.
                        Some(Err(_)) => continue,
                        None => break None,
                    }
                };

                Poll::Ready(reply)
            }
            StreamInner::Oneshot(stream) => {
                if stream.is_terminated() {
                    return Poll::Ready(None);
                }

                Pin::new(&mut *stream).poll(cx).map(|reply| {
                    reply
                        .map(|reply| Reply::new(Some(reply)).set_continues(Some(false)))
                        .ok()
                })
            }
        }
    }
}

#[derive(Debug)]
enum StreamInner<ReplyParams> {
    Broadcast(BroadcastStream<ReplyParams>),
    Oneshot(oneshot::Receiver<ReplyParams>),
}
