# Restarting without dropping clients

A service can stop gracefully, between method calls, and on Linux it can restart under systemd
without its clients noticing anything but a pause: the instance that stops hands its connections
over to the next one, which goes on with them from where they were.

## Stopping gracefully

`Server::run_until` runs a server until a future of yours completes, such as one that waits for
`SIGTERM`. It only looks at it between method calls, so the calls under way get their replies
first. The server then accepts no more connections, nor takes any more method calls, and gives back
its listener, its service and its connections, as `zlink::Stopped`:

```rust,noplayground
# async fn example<Svc: zlink::Service<zlink::tokio::unix::Stream>>(
#     server: zlink::Server<zlink::tokio::unix::Listener, Svc>,
#     sigterm: impl Future<Output = ()>,
# ) -> Result<(), Box<dyn std::error::Error>> {
// What the server gives back shuts it down as it's dropped, as it is here.
let _ = server.run_until(sigterm).await?;
# Ok(())
# }
```

A server that is given connections (`Server::with_connections`) goes on with them from where they
were, starting with the method calls that were read off them but not handled.

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
use zlink::{Connection, Server, Stopped, connection::Handover, tokio::unix::Listener};
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

    tokio::runtime::Runtime::new()?.block_on(run(passed, sigterm))
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

    let Stopped { connections, .. } = server.run_until(sigterm).await?;
    // Without a store that takes a hand-over, the service shuts down as it does without systemd.
    let Some(handover) = store.and_then(|store| store.handover().ok()) else {
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
