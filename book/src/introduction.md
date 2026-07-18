<p align="center">
  <img src="logo.svg" alt="zlink illustration" style="width: 50%;">
</p>

# Introduction

**[zlink]** is a **[Rust]** crate for **[Varlink]**. If you are not familiar with Varlink, it is a
simple *inter-process communication* (IPC) protocol: services and their clients exchange plain JSON
messages over a socket. It was designed to be the simplest feasible way to make services accessible
to both humans and machines, and it is seeing rapid adoption in the Linux plumbing layer —
[systemd] alone ships more than a dozen Varlink services these days.

Unlike [D-Bus], Varlink does not (typically) involve a broker or bus that relays messages between
peers. Clients connect *directly* to the service they want to talk to, typically through a Unix
domain socket. This point-to-point architecture is not just a simplification — as you will see
throughout this book (especially in the [Design for efficiency] chapter), it enables zlink to offer
an API that is leaner and more efficient than what is possible for a D-Bus implementation.

zlink is a 100% Rust-native, async-first implementation of the Varlink protocol. It provides:

* a low-level API to send and receive Varlink messages over a connection,
* high-level attribute macros — [`proxy`] for writing clients and [`service`] for writing
  services — that generate all the wiring for you,
* first-class support for method call [pipelining],
* runtime [introspection] and code generation from Varlink IDL, and
* `no_std` support in its core, for use in embedded environments.

## Crate organization

The zlink project is a Cargo workspace consisting of several crates:

* **[`zlink`]**: The main, unified API crate. This is the only crate you'll typically want to
  depend on directly. It re-exports the appropriate subcrates based on the enabled cargo features.
* **[`zlink-core`]**: The `no_std`-capable foundation: `Connection`, `Call`, `Reply`, `Server`,
  `Service` and friends. All the other crates build on this one.
* **[`zlink-macros`]**: The attribute and derive macros.
* **[`zlink-tokio`]**: [tokio]-based transport implementation and runtime integration.
* **[`zlink-smol`]**: [smol]-based transport implementation and runtime integration.
* **[`zlink-idl`]**: Varlink interface definition language (IDL) types and parser.
* **[`zlink-codegen`]**: Generates Rust code from Varlink IDL files.

Since `zlink` re-exports everything you need, this book will only use the `zlink` crate in its
examples. You pick your async runtime through cargo features: `tokio` (the default) or `smol`.

## Getting help

If you need help using zlink, or just want to hang out with the cool kids, please come chat with us
in the [`#zlink:matrix.org`] Matrix room. If something doesn't seem right, please [file an issue].

[zlink]: https://github.com/z-galaxy/zlink
[Rust]: https://www.rust-lang.org/
[Varlink]: https://varlink.org/
[D-Bus]: https://dbus.freedesktop.org/
[systemd]: https://systemd.io/
[Design for efficiency]: design.html
[pipelining]: pipelining.html
[introspection]: introspection.html
[`proxy`]: https://docs.rs/zlink/latest/zlink/attr.proxy.html
[`service`]: https://docs.rs/zlink/latest/zlink/attr.service.html
[`zlink`]: https://docs.rs/zlink/
[`zlink-core`]: https://docs.rs/zlink-core/
[`zlink-macros`]: https://docs.rs/zlink-macros/
[`zlink-tokio`]: https://docs.rs/zlink-tokio/
[`zlink-smol`]: https://docs.rs/zlink-smol/
[`zlink-idl`]: https://docs.rs/zlink-idl/
[`zlink-codegen`]: https://docs.rs/zlink-codegen/
[tokio]: https://tokio.rs/
[smol]: https://github.com/smol-rs/smol
[`#zlink:matrix.org`]: https://matrix.to/#/#zlink:matrix.org
[file an issue]: https://github.com/z-galaxy/zlink/issues/new
