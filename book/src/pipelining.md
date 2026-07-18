# Pipelining method calls

<!-- toc -->

Suppose you need to make ten method calls. The obvious way costs you ten round trips: send a call,
sleep until the reply wakes you up, send the next... Every one of those wake-ups is a context
switch, and as it turns out, context switches — not message parsing — are where IPC performance
goes to die.

Varlink's answer is *pipelining*: because the [specification guarantees][spec] that a service
processes calls in the order received and replies in that same order, a client can safely fire off
a whole batch of calls and then collect the replies, in order, afterwards. (Fun fact: the D-Bus
specification makes no such ordering guarantee, so a D-Bus client can never quite pull this off
safely.)

zlink treats pipelining as a first-class feature at every level of the API.

## Chaining proxy methods

For every proxy method with owned reply types, the [`proxy` macro](client.html) generates a
`chain_<method>` variant that starts a *chain*, plus a companion trait (`<TraitName>Chain`) that
lets you keep appending calls to it. Nothing hits the wire until you call `send`, which writes the
entire batch at once and hands you back a stream of replies:

```rust,no_run
use futures_util::{StreamExt, pin_mut};
use zlink::proxy;
# #[derive(Debug, serde::Deserialize)]
# struct ResolveHostnameReply { addresses: Vec<serde_json::Value> }
# #[derive(Debug, zlink::ReplyError)]
# #[zlink(interface = "io.systemd.Resolve")]
# enum ResolveError { NoNameServers }

#[proxy("io.systemd.Resolve")]
trait ResolveProxy {
    async fn resolve_hostname(
        &mut self,
        name: &str,
    ) -> zlink::Result<Result<ResolveHostnameReply, ResolveError>>;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = zlink::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    // Three DNS lookups, one write, one batch of replies.
    let replies = conn
        .chain_resolve_hostname("example.com")?
        .resolve_hostname("systemd.io")?
        .resolve_hostname("varlink.org")?
        .send::<ResolveHostnameReply, ResolveError>()
        .await?;

    pin_mut!(replies);
    while let Some(reply) = replies.next().await {
        let (reply, _fds) = reply?;
        match reply {
            Ok(reply) => println!("{:?}", reply.into_parameters()),
            Err(e) => eprintln!("error: {e:?}"),
        }
    }

    Ok(())
}
```

(This is essentially the [`resolved` example] shipped in the repository — try
`cargo run --example resolved -- example.com systemd.io`.)

Things to know about chains:

* **Replies arrive in call order** — that's the protocol guarantee that makes this work.
* **Owned reply types only.** `send::<Params, Error>()` requires `DeserializeOwned` types because
  the connection's read buffer is reused between stream items, so replies can't borrow from it.
  This is why the macro only generates `chain_` variants for methods with owned reply types (input
  parameters may still borrow all they like).
* **One reply type for the whole chain.** All replies in a chain deserialize into the same
  `Params`/`Error` type pair. When chaining different methods — even across *different interfaces
  and proxy traits* on the same connection — declare an untagged enum that covers all the possible
  reply shapes:

  ```rust,noplayground
  # #[derive(Debug, serde::Deserialize)] struct User;
  # #[derive(Debug, serde::Deserialize)] struct Post;
  #[derive(Debug, serde::Deserialize)]
  #[serde(untagged)]
  enum BlogReply {
      User(User),
      Post(Post),
  }
  ```

* **Oneway calls chain too**, and consume no reply slot: a chain of three calls where one is
  `oneway` yields exactly two replies.

## Low-level chaining

The same machinery is available on `Connection` directly, no macros involved:

```rust,noplayground
# use zlink::Call;
# async fn example(
#     conn: &mut zlink::unix::Connection,
#     get_user: Call<u32>, get_project: Call<u32>,
# ) -> zlink::Result<()> {
# #[derive(Debug, serde::Deserialize)] struct User;
# #[derive(Debug, zlink::ReplyError)]
# #[zlink(interface = "org.example")] enum ApiError { NotFound }
use futures_util::{pin_mut, stream::StreamExt};

let replies = conn
    .chain_call(&get_user, vec![])?
    .append(&get_project, vec![])?
    .send::<User, ApiError>()
    .await?;
pin_mut!(replies);
while let Some(reply) = replies.next().await {
    let (reply, _fds) = reply?;
    // ...
}
# Ok(())
# }
```

Chains have no length limit — you can `append` in a loop, or build one straight from an iterator
with `chain_from_iter` (and `chain_from_iter_with_fds` when file descriptors are involved):

```rust,noplayground
# use serde::{Serialize, Deserialize};
# use serde_prefix_all::prefix_all;
# #[prefix_all("org.example.")]
# #[derive(Debug, Serialize)]
# #[serde(tag = "method", content = "parameters")]
# enum Methods { GetUser { id: u32 } }
# #[derive(Debug, Deserialize)] struct User;
# #[derive(Debug, zlink::ReplyError)]
# #[zlink(interface = "org.example")] enum ApiError { NotFound }
# async fn example(conn: &mut zlink::unix::Connection) -> zlink::Result<()> {
let user_ids = [1, 2, 3, 4, 5];
let replies = conn
    .chain_from_iter::<Methods, _, _>(user_ids.iter().map(|&id| Methods::GetUser { id }))?
    .send::<User, ApiError>()
    .await?;
# Ok(())
# }
```

And at the very bottom sit `WriteConnection::enqueue_call` (serialize a call into the write buffer
without flushing) and `flush` (write out everything queued) — which is all a chain really is.

## What you actually save

A chain of N calls performs a single `write()` system call for the whole batch instead of N, and
the client typically gets scheduled once when the replies come in instead of N times. On the read
side, zlink drains *all* complete replies available on the socket in one `read()`, so a batch of
small replies often costs just one more syscall. The wire bytes are identical to sequential calls —
this is purely about syscalls and context switches, which dominate the cost of small-message IPC.
zlink's benchmark suite (`zlink-core/benches`) includes a sequential-vs-pipelined comparison if you
want numbers for your own machine.

The [next chapter](design.html) puts this in the broader context of zlink's design.

[spec]: https://varlink.org/Method-Call
[`resolved` example]: https://github.com/z-galaxy/zlink/blob/main/zlink/examples/resolved.rs
