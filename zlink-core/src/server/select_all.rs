use alloc::vec::Vec;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// A future that reads from multiple futures and returns the first one that is ready.
///
/// This is very similar to [`futures_util::future::SelectAll`] but much simpler and reduces
/// allocations.
pub(super) struct SelectAll<'f, Fut> {
    futures: Vec<Pin<&'f mut Fut>>,
    start_index: Option<usize>,
}

impl<'f, Fut> SelectAll<'f, Fut>
where
    Fut: Future,
{
    /// Create a new `SelectAll` with an optional starting index for round-robin polling.
    pub(super) fn new(start_index: Option<usize>) -> Self {
        SelectAll {
            futures: Vec::new(),
            start_index,
        }
    }

    /// Add a future to the `SelectAll`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the future is not moved/invalidated while it is in the
    /// `SelectAll`. The use case here is the Future impls created for `async fn` methods that are
    /// in reality `Unpin` but the compiler assumes they're `!Unpin`.
    pub(super) unsafe fn push_unchecked(&mut self, fut: &'f mut Fut) {
        self.futures.push(Pin::new_unchecked(fut))
    }
}

impl<'f, Fut> SelectAll<'f, Fut>
where
    Fut: Future + Unpin,
{
    /// Add a future to the `SelectAll`.
    pub(super) fn push(&mut self, fut: &'f mut Fut) {
        self.futures.push(Pin::new(fut));
    }
}

impl<Fut, Out> core::future::Future for SelectAll<'_, Fut>
where
    Fut: Future<Output = Out>,
{
    type Output = (usize, Out);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let num_futures = self.futures.len();
        if num_futures == 0 {
            return Poll::Pending;
        }

        let start_idx = self.start_index.map_or(0, |idx| idx % num_futures);

        for i in 0..num_futures {
            let idx = (start_idx + i) % num_futures;
            if let Poll::Ready(item) = self.futures[idx].as_mut().poll(cx) {
                return Poll::Ready((idx, item));
            }
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn round_robin_fairness() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);
        let mut future2 = ControlledFuture::new(2);

        // Make all futures ready before adding to SelectAll.
        future0.set_ready(true);
        future1.set_ready(true);
        future2.set_ready(true);

        // Test starting from index 0.
        let mut select_all = SelectAll::new(Some(0));
        select_all.push(&mut future0);
        select_all.push(&mut future1);
        select_all.push(&mut future2);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Should start from index 0.
        let pinned = Pin::new(&mut select_all);
        if let Poll::Ready((idx, value)) = pinned.poll(&mut cx) {
            assert_eq!(idx, 0);
            assert_eq!(value, 0);
        } else {
            panic!("Expected first future to be ready");
        }
    }

    #[test]
    fn round_robin_with_start_index() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);
        let mut future2 = ControlledFuture::new(2);

        // Make all futures ready before adding to SelectAll.
        future0.set_ready(true);
        future1.set_ready(true);
        future2.set_ready(true);

        // Test starting from index 1.
        let mut select_all = SelectAll::new(Some(1));
        select_all.push(&mut future0);
        select_all.push(&mut future1);
        select_all.push(&mut future2);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Should start from index 1.
        let pinned = Pin::new(&mut select_all);
        if let Poll::Ready((idx, value)) = pinned.poll(&mut cx) {
            assert_eq!(idx, 1);
            assert_eq!(value, 1);
        } else {
            panic!("Expected second future to be ready");
        }
    }

    #[test]
    fn round_robin_wrapping() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);

        // Only make first future ready before adding to SelectAll.
        future0.set_ready(true);
        future1.set_ready(false);

        // Test starting from index 1, should wrap to 0.
        let mut select_all = SelectAll::new(Some(1));
        select_all.push(&mut future0);
        select_all.push(&mut future1);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Should start from index 1, find it not ready, wrap to 0.
        let pinned = Pin::new(&mut select_all);
        if let Poll::Ready((idx, value)) = pinned.poll(&mut cx) {
            assert_eq!(idx, 0);
            assert_eq!(value, 0);
        } else {
            panic!("Expected first future to be ready after wrapping");
        }
    }

    #[test]
    fn start_index_larger_than_futures() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);

        // Make all futures ready before adding to SelectAll.
        future0.set_ready(true);
        future1.set_ready(true);

        // Test start index larger than number of futures.
        let mut select_all = SelectAll::new(Some(5)); // 5 % 2 = 1
        select_all.push(&mut future0);
        select_all.push(&mut future1);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Should start from index 1 (5 % 2).
        let pinned = Pin::new(&mut select_all);
        if let Poll::Ready((idx, value)) = pinned.poll(&mut cx) {
            assert_eq!(idx, 1);
            assert_eq!(value, 1);
        } else {
            panic!("Expected second future to be ready");
        }
    }

    #[test]
    fn no_start_index_defaults_to_zero() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);
        let mut future2 = ControlledFuture::new(2);

        // Make all futures ready before adding to SelectAll.
        future0.set_ready(true);
        future1.set_ready(true);
        future2.set_ready(true);

        // Test with None start index.
        let mut select_all = SelectAll::new(None);
        select_all.push(&mut future0);
        select_all.push(&mut future1);
        select_all.push(&mut future2);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Should start from index 0 by default.
        let pinned = Pin::new(&mut select_all);
        if let Poll::Ready((idx, value)) = pinned.poll(&mut cx) {
            assert_eq!(idx, 0);
            assert_eq!(value, 0);
        } else {
            panic!("Expected first future to be ready");
        }
    }

    #[test]
    fn empty_select_all_returns_pending() {
        let mut select_all = SelectAll::<ControlledFuture>::new(Some(0));
        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        let pinned = Pin::new(&mut select_all);
        assert!(matches!(pinned.poll(&mut cx), Poll::Pending));
    }

    #[test]
    fn all_futures_pending() {
        let mut future0 = ControlledFuture::new(0);
        let mut future1 = ControlledFuture::new(1);

        let mut select_all = SelectAll::new(Some(1));
        select_all.push(&mut future0);
        select_all.push(&mut future1);

        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        // Don't make any futures ready.
        let pinned = Pin::new(&mut select_all);
        assert!(matches!(pinned.poll(&mut cx), Poll::Pending));
    }

    /// A controllable future that can be made ready on demand.
    struct ControlledFuture {
        ready: bool,
        value: usize,
    }

    impl ControlledFuture {
        fn new(value: usize) -> Self {
            Self {
                ready: false,
                value,
            }
        }

        fn set_ready(&mut self, ready: bool) {
            self.ready = ready;
        }
    }

    impl Future for ControlledFuture {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.ready {
                Poll::Ready(self.value)
            } else {
                Poll::Pending
            }
        }
    }

    /// Creates a dummy waker for testing.
    fn dummy_waker() -> Waker {
        use core::task::{RawWaker, RawWakerVTable};

        fn dummy_raw_waker() -> RawWaker {
            RawWaker::new(core::ptr::null(), &VTABLE)
        }

        const VTABLE: RawWakerVTable =
            RawWakerVTable::new(|_| dummy_raw_waker(), |_| {}, |_| {}, |_| {});

        unsafe { Waker::from_raw(dummy_raw_waker()) }
    }
}
