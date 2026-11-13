# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.9.0 - 2026-11-13

### ✨ Added
- Add the `byte_stream` module, for connections over any byte stream.
- Add block_on, to drive zruntime on the calling thread.
- Turn std sockets into streams on local runtimes too.
- Implement IntoStd for the Unix stream.
- Add the notified state.
- Add the Unix domain socket transport.
- Add a crate for zruntime integration.

### 📦 Dependencies
- Depend on zruntime 0.2.3.
- Depend on futures-util.

### 📝 Documentation
- Document the byte_stream module. #357
