//! Tests for the state that the methods of an interface borrow from the service it is part of.

use serde::{Deserialize, Serialize};
use zlink::{
    Call, Connection, introspect,
    service::{Interface, MethodReply},
    test_utils::mock_socket::MockSocket,
};

#[test_log::test(tokio::test)]
async fn methods_borrow_the_state() {
    let mut counter = Counter { increments: 0 };
    let mut count = 40;

    let json = r#"{"method":"org.example.counter.Increment","parameters":{"by":2}}"#;
    assert_eq!(
        dispatch(&mut counter, &mut count, json).await,
        r#"{"count":42}"#
    );
    assert_eq!(count, 42);
    assert_eq!(counter.increments, 1);

    // The reply borrows from the state.
    let json = r#"{"method":"org.example.counter.Counts"}"#;
    assert_eq!(
        dispatch(&mut counter, &mut count, json).await,
        r#"{"counts":[42]}"#
    );

    // A method with the connection gets the state too.
    let json = r#"{"method":"org.example.counter.Reset"}"#;
    assert_eq!(
        dispatch(&mut counter, &mut count, json).await,
        r#"{"count":42}"#
    );
    assert_eq!(count, 0);
}

/// Dispatch a call, given as JSON, to `counter` with `count` as the state, and return the JSON of
/// the parameters of its reply.
async fn dispatch(counter: &mut Counter, count: &mut u64, json: &str) -> String {
    let method: <Counter as Interface<MockSocket, u64>>::MethodCall<'_> =
        serde_json::from_str(json).unwrap();
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let (reply, _fds) = <Counter as Interface<MockSocket, u64>>::handle(
        counter,
        count,
        Call::new(&method),
        &mut conn,
        Vec::new(),
    )
    .await;
    let MethodReply::Single(Some(params)) = reply else {
        panic!("{json} wasn't replied to with parameters");
    };

    serde_json::to_string(&params).unwrap()
}

/// One interface of a counting service, which shares the count with the other interfaces.
struct Counter {
    increments: u64,
}

#[zlink::interface("org.example.counter")]
impl<Sock> Counter {
    /// Add `by` to the count.
    async fn increment(&mut self, by: u64, #[zlink(state)] count: &mut u64) -> Count {
        self.increments += 1;
        *count += by;

        Count { count: *count }
    }

    /// The counts, of which there's only one.
    async fn counts<'a>(&'a self, #[zlink(state)] count: &'a u64) -> Counts<'a> {
        Counts {
            counts: std::slice::from_ref(count),
        }
    }

    /// Reset the count, returning the one before.
    async fn reset(
        &mut self,
        #[zlink(connection)] conn: &mut Connection<Sock>,
        #[zlink(state)] count: &mut u64,
    ) -> Count {
        let _ = conn;

        Count {
            count: std::mem::take(count),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Count {
    count: u64,
}

#[derive(Debug, Serialize, introspect::Type)]
struct Counts<'a> {
    counts: &'a [u64],
}
