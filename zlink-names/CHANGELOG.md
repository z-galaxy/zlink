# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.9.0 - 2026-11-13

### 📝 Documentation
- Document the byte_stream module. #357
- Document connections over any byte stream.
- Write services as interfaces that a struct holds.
- Drive the examples of the README with zlink::zruntime::block_on.
- Document all three runtime features.
- Document the zruntime runtime support.

## 0.8.0 - 2026-10-29

### ✨ Added
- Implement serde traits for the name types.
- Add a crate for Varlink names.

### ♻️ Changed
- Move the Varlink name grammar out of zlink-idl.

### 📦 Dependencies
- Add an optional dependency on serde.
- Depend on zcheapstr.
- Depend on winnow.

### ⚙️ Other
- 🏷️ Add types for Varlink names.
