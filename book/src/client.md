# Writing a client proxy

<!-- toc -->

In this chapter, we are going to see how to write a convenient, type-safe client for a Varlink
service using the `proxy` attribute macro.

To make this learning "hands-on", we are going to talk to a real service: `systemd-resolved`, the
DNS resolver service of systemd, which exposes the `io.systemd.Resolve` Varlink interface.

Let's start by playing with the service from the shell with [`varlinkctl`]:

```bash
varlinkctl call /run/systemd/resolve/io.systemd.Resolve \
  io.systemd.Resolve.ResolveHostname '{"name": "systemd.io", "family": 2}'
```

You should get something like:

```json
{
        "addresses" : [
                {
                        "ifindex" : 2,
                        "family" : 2,
                        "address" : [ 185, 199, 111, 153 ]
                }
        ],
        "name" : "systemd.io",
        "flags" : 1048577
}
```

This command shows us all the elements of a Varlink method call:

- **address**: the path of the socket the service listens on
  (`/run/systemd/resolve/io.systemd.Resolve`).
- **method**: the fully-qualified method name — interface (`io.systemd.Resolve`) + method
  (`ResolveHostname`).
- **parameters**: a JSON object. Since the wire format *is* JSON, what you type is exactly what the
  service receives.

We saw in the [previous chapter](connection.html#low-level-method-calls) how to make such a call
with the low-level `Connection` API. It works, but declaring method enums and matching up reply
types by hand is verbose and error-prone. Let's have a macro do it for us.

## Trait-derived proxy

The `proxy` attribute macro takes a trait declaration and implements it **directly on
`Connection`**:

```rust,no_run
use serde::Deserialize;
use zlink::{proxy, ReplyError};

#[proxy("io.systemd.Resolve")]
trait ResolveProxy {
    async fn resolve_hostname(
        &mut self,
        name: &str,
    ) -> zlink::Result<Result<ResolveHostnameReply, ResolveError>>;
}

#[derive(Debug, Deserialize)]
struct ResolveHostnameReply {
    addresses: Vec<ResolvedAddress>,
}

#[derive(Debug, Deserialize)]
struct ResolvedAddress {
    family: i32,
    address: Vec<u8>,
}

#[derive(Debug, ReplyError)]
#[zlink(interface = "io.systemd.Resolve")]
enum ResolveError {
    NoNameServers,
    NoSuchResourceRecord,
    QueryTimedOut,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = zlink::tokio::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    // The proxy methods are available directly on the connection.
    match conn.resolve_hostname("systemd.io").await? {
        Ok(reply) => println!("{:?}", reply.addresses),
        Err(e) => eprintln!("lookup failed: {e:?}"),
    }

    Ok(())
}
```

If you're coming from zbus, notice what's *missing* here: there is no proxy object. In zbus you
create a `Proxy` value that wraps a (shared) connection; in zlink the generated trait is
implemented for `Connection<S>` itself, so once the trait is in scope, you call interface methods
directly on your connection. A connection *is* a client. (The flip side: since the trait needs to
be in scope, be mindful of what you import — two traits declaring the same method name will require
disambiguation.)

The snake_case method names are automatically converted to PascalCase for the wire format
(`resolve_hostname` → `ResolveHostname`). Use `#[zlink(rename = "...")]` on a method when the
default mapping doesn't fit.

A proxy method must:

- take `&mut self` (remember: exclusive access, no locks — see
  [Design for efficiency](design.html)),
- return `zlink::Result<Result<ReplyParams, ReplyError>>` — the same nested-`Result` shape we met
  in the previous chapter: transport-level errors on the outside, method errors from the service on
  the inside,
- use parameter types that implement `Serialize`, and reply/error types that implement
  `Deserialize`.

Reply types may borrow from the connection's read buffer for zero-copy deserialization — just give
them a lifetime parameter (e.g. `Result<Status<'_>, MyError<'_>>`). We used owned types (`Vec<u8>`)
above for simplicity.

## Reply errors

The `ReplyError` derive macro maps a Rust enum to Varlink error replies of an interface. The
mandatory `interface` attribute provides the reverse-domain prefix; the variant name is the error
name; named fields become the error's parameters:

```rust,noplayground
use zlink::ReplyError;

#[derive(Debug, ReplyError)]
#[zlink(interface = "org.example.ftl")]
enum FtlError {
    // -> {"error": "org.example.ftl.NotEnoughEnergy"}
    NotEnoughEnergy,
    // -> {"error": "org.example.ftl.ParameterOutOfRange", "parameters": {"field": "..."}}
    ParameterOutOfRange { field: String },
}
```

Variant and field names can be adjusted with `#[zlink(rename = "...")]` / `rename_all`, and string
fields can borrow zero-copy with `#[zlink(borrow)]` on a `Cow<'_, str>` field. Tuple variants are
not supported — Varlink error parameters are named, so use struct variants.

## Streaming calls

Where D-Bus has signals and properties-changed notifications, Varlink has
[`more` calls](concepts.html#method-calls): a single call that yields a stream of replies. On the
proxy side, mark the method with `#[zlink(more)]` and return a `Stream`:

```rust,noplayground
use futures_util::Stream;
use zlink::proxy;
# #[derive(Debug, serde::Deserialize)]
# struct DriveCondition { state: String }
# #[derive(Debug, zlink::ReplyError)]
# #[zlink(interface = "org.example.ftl")]
# enum FtlError { NotEnoughEnergy }

#[proxy("org.example.ftl")]
trait FtlProxy {
    #[zlink(more)]
    async fn monitor_drive_condition(
        &mut self,
    ) -> zlink::Result<impl Stream<Item = zlink::Result<Result<DriveCondition, FtlError>>>>;
}
```

Each stream item is one reply. Note that the reply and error types of a streaming method must be
owned (`DeserializeOwned`): the connection's read buffer is reused from one stream item to the
next, so borrowed data can't outlive a single iteration.

Since a streaming call occupies its connection for as long as the stream lives, the idiomatic
pattern is to dedicate a connection to each subscription and open another one for your
request/response traffic. Connections are cheap — this is the Varlink way. ✨

## Other method attributes

- `#[zlink(oneway)]`: fire-and-forget; the method must return `zlink::Result<()>` since there is no
  reply at all.
- `#[zlink(rename = "...")]` on a parameter: customize the wire name of that parameter.
- `#[zlink(fds)]` on a `Vec<OwnedFd>` parameter: pass file descriptors along with the call.
  `#[zlink(return_fds)]` on the method makes it also return the descriptors that came with the
  reply. As on D-Bus, the convention is for the JSON parameters to carry *indexes* into the
  descriptor array.
- Methods can be generic over their parameter types, with the usual `Serialize` bounds.

## Pipelining

The macro additionally generates a `chain_<method>` variant of every method with owned reply types,
letting you batch several calls into a single write and read the replies as a stream. That deserves
its own chapter: see [Pipelining method calls](pipelining.html).

## Generating the proxy from IDL

If the service you're binding publishes its IDL (via introspection or as a `.varlink` file), you
don't have to write any of the above by hand — [`zlink-codegen`](introspection.html#code-generation)
can generate the trait, types, and error enums for you.

[`varlinkctl`]: https://www.freedesktop.org/software/systemd/man/latest/varlinkctl.html
