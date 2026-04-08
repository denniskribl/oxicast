# Contributing to oxicast

Thanks for your interest in contributing!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/your-username/oxicast`
3. Create a branch: `git checkout -b my-feature`
4. Install protobuf compiler: `brew install protobuf` (macOS) or `apt install protobuf-compiler` (Linux)
5. Make your changes
6. Run all quality gates (see below)
7. Submit a PR

## Quality Gates

All of these must pass before merging. CI enforces them automatically.

```sh
cargo check --all-features
cargo check --no-default-features
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo doc --no-deps --all-features   # with RUSTDOCFLAGS=-Dwarnings
cargo check --examples --all-features
```

## Development

```sh
# Run all tests
cargo test --all-features

# Run a specific test
cargo test test_name

# Run an example against a real Chromecast
cargo run --example device_test --all-features -- 192.168.1.100

# With protocol tracing
RUST_LOG=oxicast=trace cargo run --example device_test --all-features

# Build and preview documentation site
cd website && pnpm install && pnpm dev
```

## Guidelines

- Follow existing code style (`cargo fmt`)
- Add tests for new functionality
- Update documentation for public API changes
- Keep PRs focused — one feature or fix per PR
- Don't add dependencies without discussion
- All public types need doc comments (`#![warn(missing_docs)]` is enforced)

## Release Process

Releases are triggered by pushing a `v*` tag. Before tagging:

1. Update version in `Cargo.toml`
2. Add a `## [x.y.z]` heading to `CHANGELOG.md`
3. Commit: `git commit -m "release: vx.y.z"`
4. Tag: `git tag vx.y.z`
5. Push: `git push && git push --tags`

CI will verify, publish to crates.io, and create a GitHub release.
