# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.8.0 - 2026-10-29

### 💥 Breaking
- Name fields and enum variants with FieldName.
- Name types, methods and errors with TypeName.
- Name interfaces with InterfaceName.

### ♻️ Changed
- Move the Varlink name grammar out of zlink-idl.

### 📦 Dependencies
- Depend on zlink-names.

### 📝 Documentation
- List zlink-names among the project's crates.
- Link to the file descriptor passing docs.

### 🐛 Fixed
- Fix validating of field name. #309

### ⚙️ Other
- 💸 Add CodSpeed as a sponsor in the README.

## 0.7.1 - 2026-09-09

### 🐛 Fixed
- Reject trailing hyphens in interface name segments.
- Comma-separate variants of multi-line inline enums.

## 0.7.0 - 2026-07-19

### Added
- ✨ Add const field-list helpers for flatten codegen.

### Changed
- 🏗️ Split the IDL types and parser into their own crate. #78

