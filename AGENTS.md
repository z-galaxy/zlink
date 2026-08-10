# AGENTS.md

This file provides guidance to AI coding agents when working with code in this repository.

For contribution conventions — commit-message format, atomic commits, code layout, and
more — follow the guidelines in [`CONTRIBUTING.md`](CONTRIBUTING.md).

- **Changelog-skip trailer**: end a commit message with a `Changelog: skip` git trailer to
  keep it out of the user-facing changelog (use for AI-workflow artifacts such as design docs
  and implementation plans).

## Development Commands

### Testing
```bash
# Run the full test suite, including doc tests and compile-tests
cargo test --all-features
# For no_std
cargo test -p zlink-core --no-default-features --features idl-parse,proxy,defmt
```

### Code Quality
```bash
# Format code (uses nightly rustfmt)
cargo +nightly fmt --all

# Run clippy with warnings as errors
cargo clippy -- -D warnings

# Check all features compile
cargo check --all-features
# For no_std
cargo check -p zlink-core --no-default-features --features idl-parse,proxy,defmt
```

### Testing crates in isolation

We only support running tests in isolation in `zlink`, `zlink-core` and `zlink-codegen` (e.g
`cargo -p zlink-macros test` is not supported and allowed to fail).

### Git Hooks Setup
```bash
# Enable git hooks for automatic formatting and clippy checks
cp .githooks/* .git/hooks/
```

## Architecture Overview

This is a Rust workspace implementing an asynchronous Varlink IPC library. The architecture is modular with clear separation of concerns:

### Core Architecture
- **zlink-core**: No-std foundation providing core APIs. Not used directly.
- **zlink-macros**: Contains the attribute and derive macros. Not used directly.
- **zlink-tokio**: Tokio runtime integration and transport implementations. Not used directly.
- **zlink-smol**: smol runtime integration and transport implementations. Not used directly.
- **zlink**: Main unified API crate that re-exports appropriate subcrates based on cargo features.

### Key Components
- **Connection**: Low-level API for message send/receive with unique IDs for read/write halves
- **Server**: Listens for connections and handles method calls via services
- **Service**: Trait defining IPC service implementations
- **Call/Reply**: Core message types for IPC communication

### Feature System

#### Main Features

- `tokio` (default): Enable tokio runtime integration and use of standard library.
- `server` (default): Enable server-related functionality (Server, Listener, Service).
- `proxy` (default): Enable the `#[proxy]` macro for type-safe client code.
- `tracing` (default): Enable `tracing`-based logging.
- `defmt`:  Enable `defmt`-based logging. If both `tracing` and `defmt` is enabled, `tracing` is
  used.

#### IDL and Introspection

- `idl`: Support for IDL type representations.
- `introspection`: Enable runtime introspection of service interfaces.
- `idl-parse`: Parse Varlink IDL at runtime (works in `no_std`).

### Development Patterns
- Uses workspace-level package metadata (edition, rust-version, license, repository)
- zlink-core supports both std and no_std environments through feature flags
- Uses pin-project-lite for async/await support
- Only enable needed features of dependencies
- For logging, use the macros from `log` module that abstract over tracing and defmt

## Testing Infrastructure

### Mock Socket API
Use consolidated mock socket utilities from `zlink-core/src/test_utils/mock_socket.rs`:
- `MockSocket::new(&responses)` - full socket with pre-configured responses
- `TestWriteHalf::new(expected_len)` - validates exact write lengths
- `CountingWriteHalf::new()` - counts write operations for pipelining tests
