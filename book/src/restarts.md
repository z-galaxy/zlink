# Restarting without dropping clients

A service can stop gracefully, between method calls, and on Linux it can restart under systemd
without its clients noticing anything but a pause: the instance that stops hands its connections
over to the next one, which goes on with them from where they were.

## Stopping gracefully

`Server::run_until` runs a server until a future of yours completes, such as one that waits for
`SIGTERM`. It checks the future between method calls, so the calls in progress are handled first,
and while it waits to write to a client, so a client that doesn't read its replies can't block the
shutdown. The server then stops accepting connections and reading method calls, and returns its
listener, its service and its connections, as `zlink::Stopped`. If it interrupted a reply that the
socket couldn't take at once, the unwritten part stays in the connection, which writes it when it's
flushed. A service that shuts down can give its clients a moment to read it:

```rust,noplayground
# async fn example<Svc: zlink::Service<zlink::zruntime::unix::Stream>>(
#     server: zlink::Server<zlink::zruntime::unix::Listener, Svc>,
#     sigterm: impl Future<Output = ()>,
# ) -> Result<(), Box<dyn std::error::Error>> {
use std::time::Duration;

// What the server returns shuts it down when it's dropped, as it is here once the clients had a
// second at most to read the rest of their replies.
let mut stopped = server.run_until(sigterm).await?;
let flushes = async {
    for conn in &mut stopped.connections {
        // A client that went away has nothing left to read.
        let _ = conn.flush().await;
    }
};
let _ = zruntime::SharedRuntime::current()?.timeout(Duration::from_secs(1), flushes).await;
# Ok(())
# }
```

This timeout uses the [zruntime] crate. Add it to your `Cargo.toml`, or use the timeout API of your
runtime.

A server that is given connections (`Server::with_connections`) continues them from where they
were: it first writes what they still have to write, and then handles the method calls that were
read but not handled.

## Restarting under systemd

With the `handover` feature, `Connection::hand_over` gives a connection up as a
`zlink::connection::Handover`: its socket, what was read off it and not received yet, with the
credentials that came with it, and what was to be written to it and wasn't yet, along with their
file descriptors. [zoter] stores the hand-overs of a service, and its state, in the file descriptor
store that systemd keeps for the service across its restarts, and gives them back in the next
instance, where `Connection::take_over` goes on with each. Taking the file descriptors that systemd
passes changes the environment, which no other thread may read or write meanwhile, so the service
takes them before its runtime starts any:

```rust,noplayground
use std::time::Duration;

use zlink::{Connection, Server, Stopped, connection::Handover, zruntime::unix::Listener};
use zoter::{Passed, Store};

# #[derive(Debug)]
# struct Greeter;
# #[derive(Debug, serde::Serialize, serde::Deserialize, zlink::introspect::Type)]
# struct Greeting { greeting: String }
# #[zlink::service(interface = "org.zlink.MyGreeter")]
# impl Greeter {
#     async fn say_hello(&self, name: String) -> Greeting {
#         Greeting { greeting: format!("Hello {name}!") }
#     }
# }
// What `main` comes down to, with `sigterm` completing once the service is told to stop.
fn start(sigterm: impl Future<Output = ()>) -> Result<(), Box<dyn std::error::Error>> {
    // The next instance takes the file descriptors of the hand-over under this limit.
    zoter::raise_fd_limit()?;
    // SAFETY: No runtime has started yet, so no other thread runs to read or write the environment
    // meanwhile, and nothing took the file descriptors that systemd passed.
    let passed = unsafe { Passed::from_env() }?;

    zlink::zruntime::block_on(run(passed, sigterm))
}

async fn run(
    mut passed: Passed,
    sigterm: impl Future<Output = ()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::from_env("greeter")?;
    let restored = match &mut store {
        Some(store) => store.take_over(&mut passed)?,
        None => None,
    };
    // The socket of socket activation, which systemd keeps open while the service restarts.
    let socket = passed.take("greeter.socket").ok_or("no socket to listen on")?;
    let mut server = Server::new(Listener::try_from(socket)?, Greeter);
    if let Some(mut restored) = restored {
        let connections = restored
            .take_items()
            .map(|item| Connection::take_over(item.restore::<Handover>()?))
            .collect::<zlink::Result<Vec<_>>>()?;
        server = server.with_connections(connections);
    }
    if let Some(store) = &store {
        store.notifier().notify("READY=1")?;
    }

    let Stopped { mut connections, .. } = server.run_until(sigterm).await?;
    // Without a store that takes a hand-over, the service shuts down as it does without systemd,
    // giving its clients a second at most to read the rest of their replies.
    let Some(handover) = store.and_then(|store| store.handover().ok()) else {
        let flushes = async {
            for conn in &mut connections {
                let _ = conn.flush().await;
            }
        };
        let _ = zruntime::SharedRuntime::current()?
            .timeout(Duration::from_secs(1), flushes)
            .await;

        return Ok(());
    };
    let handovers = connections
        .into_iter()
        .map(Connection::hand_over)
        .collect::<zlink::Result<Vec<Handover>>>()?;
    // Each connection goes under an id of its own, which the state could refer to, if it had to.
    handover.store(&[][..], (0..).zip(&handovers))?;

    Ok(())
}
```

The unit of the service sets `FileDescriptorStoreMax=` for systemd to keep a store, and a socket
unit has systemd keep the socket that the service listens on, and the clients that connect while no
instance runs. zoter's documentation tells more.

The connections whose replies still stream (methods called with `more`), and those that were
upgraded to another protocol, are closed as the server stops: nothing generic can carry on with
them.

[zoter]: https://github.com/z-galaxy/zoter
[zruntime]: https://github.com/z-galaxy/zruntime
