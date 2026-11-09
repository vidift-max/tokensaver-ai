# Foundation verification

The Rust foundation must be validated in CI on every push and pull request.

Required checks:

- cargo check --manifest-path src-tauri/Cargo.toml
- cargo test --manifest-path src-tauri/Cargo.toml
- npm ci
- npm run build

Security regression cases belong in Rust unit/integration tests and must cover path traversal, invalid UTF-8, file-size limits, deduplication, reimport versioning, and paginated reads.
