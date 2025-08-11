# Contributing to The One

First off, thank you for considering contributing. Every contribution helps to make this project better.

## How to Contribute

We welcome contributions in many forms, including bug reports, feature requests, documentation improvements, and code contributions.

### Reporting Bugs

If you find a bug, please create an issue in the GitHub repository. Be sure to include:

*   A clear and descriptive title.
*   A detailed description of the problem, including steps to reproduce it.
*   The expected behavior and what actually happened.
*   Your environment details (OS, Rust version, Node.js version).

### Submitting Changes

1.  Fork the repository and create a new branch from `master`.
2.  Make your changes.
3.  Ensure your code adheres to the project's code style by running `cargo fmt`.
4.  Run the test suite to ensure your changes haven't broken anything.
5.  Commit your changes with a clear and concise commit message.
6.  Push your branch to your fork and open a pull request.

## Code Style

This project follows the standard Rust formatting guidelines enforced by `rustfmt`. Before submitting a pull request, please ensure your code is formatted by running:

```bash
cargo fmt
```

We also use `clippy` for linting. Please run `cargo clippy` and address any warnings before submitting.

## Testing Strategy(TODO)

This project contains two primary Rust artifacts—a WebAssembly library for the Cloudflare Worker and a native binary for the `sync-cli`—which require different testing approaches.

### Testing the Worker Library (`cdylib`)

The Worker's HTTP interface is tested using an integration-style approach.

*   **Framework**: We use `axum-test` to create an in-memory test server that can receive mock HTTP requests. This allows us to test the full request/response lifecycle of our Axum router without needing a live network connection.
*   **Mocking**: External services and APIs are mocked using the `mockito` crate. This ensures our tests are fast, reliable, and independent of external factors.
*   **Location**: Tests for the worker library are located within the `crates/theone-balance/src/lib.rs` and its modules.

### Testing the CLI Binary (`sync-cli`)

The `sync-cli` is tested using standard Rust binary testing patterns.

*   **Framework**: We use the `assert_cmd` crate to execute the compiled CLI binary as a separate process.
*   **Assertions**: `assert_cmd` allows us to make assertions about the binary's exit code, `stdout`, and `stderr`, verifying that the CLI behaves correctly under different conditions.
*   **Location**: Tests for the CLI are located in the `crates/theone-balance/src/bin/sync_cli.rs` file.

### Running All Tests

You can run the entire test suite for both the library and the binary from the workspace root with a single command:

```bash
cargo test --all
```
