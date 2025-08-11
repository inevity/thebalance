# How to Get the API of a Library Crate in Rust

## Overview

When you need to use a Rust library crate in your project, discovering its public API is essential. This guide shows you exactly how to extract and understand what functions, structs, traits, and other items a crate exposes for external use.

## Quick Start: The Essential Steps

### 1. Start with the Crate Root (`src/lib.rs`)
This file defines everything that's publicly available from the crate:

```rust
// src/lib.rs - The API gateway
pub mod http_client;           // Public module
pub mod errors;               // Public module

pub use http_client::Client;   // Re-exported for convenience
pub use errors::HttpError;     // Re-exported error type

pub struct Config {            // Public struct
    pub timeout: u64,
    pub retries: u32,
}

pub trait Requestable {        // Public trait
    fn make_request(&self) -> Result<Response, HttpError>;
}

pub fn create_default_client() -> Client {  // Public function
    // ...
}

pub const DEFAULT_TIMEOUT: u64 = 30;  // Public constant
```

### 2. Generate Documentation
```bash
# This creates browsable HTML documentation
cargo doc --open

# For local development, include all details
cargo doc --open --document-private-items
```

### 3. Examine the Public Module Tree
Follow the `pub mod` declarations to understand the crate's structure:

```rust
// If you see: pub mod http_client;
// Check: src/http_client.rs or src/http_client/mod.rs

// Look for public items in each module:
pub struct Client { }          // Available as crate_name::http_client::Client
pub fn connect() -> Client { } // Available as crate_name::http_client::connect
```

## Systematic API Extraction

### Method 1: Documentation-First Approach
```bash
# Add the crate to your Cargo.toml
[dependencies]
target_crate = "1.0"

# Generate documentation for your project
cargo doc --open

# Navigate to the crate's documentation in the browser
```

### Method 2: Source Code Analysis
```bash
# Search for all public items
rg "^pub " src/

# Find specific item types
rg "pub fn " src/          # Public functions
rg "pub struct " src/      # Public structs  
rg "pub enum " src/        # Public enums
rg "pub trait " src/       # Public traits
rg "pub use " src/         # Re-exports
rg "pub const " src/       # Public constants
rg "pub static " src/      # Public statics
```

### Method 3: Using Cargo Tools
```bash
# Install and use cargo-public-api
cargo install cargo-public-api
cargo public-api

# Alternative: Use rustdoc JSON output
cargo +nightly rustdoc -- -Z unstable-options --output-format json
```

## Understanding What You Find

### Public vs Private Items
```rust
pub fn public_function() { }        // ✅ Available externally
pub(crate) fn crate_function() { }  // ❌ Only within this crate
pub(super) fn parent_function() { } // ❌ Only in parent module
fn private_function() { }           // ❌ Private to this module
```

### Re-exports and Module Paths
```rust
// In src/lib.rs
pub use network::http::Client;  // Now available as: crate_name::Client
pub use network::http;          // Now available as: crate_name::http::*

// Without re-export, you'd need: crate_name::network::http::Client
// With re-export, you can use: crate_name::Client
```

### Feature-Gated APIs
Check `Cargo.toml` for optional features:
```toml
[features]
default = ["std"]
async = ["tokio"]
tls = ["rustls"]
```

Then look for conditional APIs:
```rust
#[cfg(feature = "async")]
pub async fn async_request() -> Result<Response, Error> { }

#[cfg(feature = "tls")]
pub struct TlsConfig { }
```

## Practical API Discovery Workflow

### Step 1: Get the Crate
```bash
# Add to your Cargo.toml dependencies
my_crate = "1.0"

# Or clone the source
git clone https://github.com/user/my_crate.git
cd my_crate
```

### Step 2: Generate API Overview
```bash
cargo doc --open --no-deps
```

### Step 3: Examine Key Files
```bash
# Start with the main API definition
cat src/lib.rs

# Check for examples
ls examples/
cat examples/basic_usage.rs

# Look at integration tests (they show external usage)
ls tests/
cat tests/integration_test.rs
```

### Step 4: Search for Patterns
```bash
# Find all public APIs
rg "^pub " src/ | head -20

# Find macro exports
rg "#\[macro_export\]" src/

# Check for prelude modules
find src/ -name "prelude.rs"
```

### Step 5: Verify with Usage
Create a test file to confirm your understanding:
```rust
// test_api.rs
use my_crate::{
    Client,           // Public struct
    Config,          // Public struct  
    HttpError,       // Public enum
    Requestable,     // Public trait
    create_client,   // Public function
    DEFAULT_TIMEOUT, // Public constant
};

fn main() {
    let config = Config {
        timeout: DEFAULT_TIMEOUT,
        retries: 3,
    };
    
    let client = create_client(config);
    // This compiles = API is correctly identified
}
```

## Common API Patterns in Rust Crates

### Constructor Patterns
```rust
impl MyStruct {
    pub fn new() -> Self { }                    // Basic constructor
    pub fn with_config(config: Config) -> Self { } // Configured constructor
    pub fn builder() -> MyStructBuilder { }      // Builder pattern
}
```

### Error Handling
```rust
pub enum MyError {      // Public error enum
    Network(String),
    Parse(String),
}

pub type Result<T> = std::result::Result<T, MyError>;  // Convenience type
```

### Trait Definitions
```rust
pub trait Processable {     // Public trait for external implementation
    fn process(&self) -> Result<Output>;
}
```

## Tools and IDE Integration

### VS Code with rust-analyzer
- Hover over items to see their documentation and visibility
- Use `Ctrl+Click` to jump to definitions
- The outline view shows public/private items

### Command Line Tools
```bash
# Install useful tools
cargo install cargo-expand      # See macro expansions
cargo install cargo-public-api  # Extract public API
cargo install cargo-modules     # Visualize module structure

# Usage
cargo expand                    # Show expanded macros
cargo public-api               # List public API items
cargo modules generate tree    # Show module hierarchy
```

## Validation Checklist

✅ **Found the main API surface** - Items declared in `src/lib.rs`  
✅ **Identified re-exports** - Convenient access paths  
✅ **Discovered feature flags** - Optional functionality  
✅ **Discovered macro generateed api** - Optional functionality  
✅ **Located examples** - Real usage patterns  
✅ **Found error types** - How the crate reports failures  
✅ **Understood traits** - Extension points for your code  
✅ **Tested compilation** - Verified APIs actually work  

## Quick Reference: Files to Check

1. **`src/lib.rs`** - Primary API definitions
2. **`Cargo.toml`** - Features and metadata
3. **`examples/`** - Usage examples
4. **`tests/`** - Integration test patterns
5. **`README.md`** - Quick start guide
6. **`docs.rs`** - Online documentation

## Summary

Getting a Rust crate's API involves:
1. **Starting with `src/lib.rs`** to see the public interface
2. **Generating documentation** with `cargo doc --open`
3. **Searching for `pub` items** in the source code
4. **Testing your understanding** by using the APIs in code
5. **Checking examples and tests** for usage patterns

The key insight is that `src/lib.rs` acts as the definitive catalog of everything the crate makes available to external users. Everything else helps you understand how to use those APIs effectively.
