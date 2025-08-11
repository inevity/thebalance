# The Balance

[![MIT License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](http://makeapullrequest.com)

An intelligent API gateway for routing requests to AI providers, built with Rust and running on Cloudflare Workers. It provides a resilient and observable interface for managing and using multiple provider API keys.

## Overview

This project is a Rust-based monorepo structured as a Cargo workspace. Its core purpose is to act as a highly available API gateway that manages a pool of API keys for AI providers (specifically Google Gemini, based on current implementation) and intelligently routes incoming requests through them.

The project exposes several distinct interfaces:

1.  **AI Gateway API (`/api/*`)**: The primary function of the worker is a sophisticated reverse proxy that intelligently handles different types of API requests. Its behavior adapts based on the environment (Production vs. Local Development).

    The gateway supports three distinct patterns:

    *   **A) OpenAI-Compatible Chat (`/api/compat/chat/completions`)**
        *   **Production:** Forwards the OpenAI-formatted request directly to the Cloudflare AI Gateway, relying on the gateway for translation to the native provider's API.
        *   **Local Development:** The worker's built-in translation layer converts the OpenAI request to the native Google Gemini format before sending it to the provider's actual endpoint. It then translates the response back.

    *   **B) OpenAI-Compatible Embeddings (`/api/compat/embeddings`)**
        *   **Production & Local Development:** The worker's built-in translation layer is active in *both* environments. It converts the OpenAI request body to the native Gemini format, constructs the corresponding native provider path, and sends it onward. It then translates the response back to the OpenAI format.

    *   **C) Provider-specific API Proxy (`/api/{provider}/*`)**
        *   This mode allows clients to use a provider's native API and SDKs directly. The gateway intercepts these native requests, injects the healthiest available API key from its managed pool into the authentication header, and then forwards the request. This provides the benefit of the gateway's key management, resilience, and failover logic while still allowing the use of native provider features.
        *   **Production:** Forwards the newly authenticated request to the Cloudflare AI Gateway's provider-specific API endpoint.
        *   **Local Development:** Forwards the newly authenticated request directly to the provider's actual endpoint (e.g., `generativelanguage.googleapis.com`).

2.  **Key Management UI**: A web interface for managing the pool of API keys. It allows users to view keys by provider, add new keys in bulk, delete keys, and run validation tests.
3.  **Key Management API**: A set of endpoints used by the UI (and available for programmatic use) to manage keys:
    *   `POST /api/keys/add/{provider}`: Adds one or more new keys for a specific provider.
    *   `GET /api/keys/{id}/coolings`: Retrieves the detailed cooldown status for a single key.
4.  **Administrative API (`/test/run-cleanup/*`)**: An endpoint to manually trigger the background process that cleans up and deletes permanently failed but active keys.

The project also includes a command-line tool:

*   **Sync CLI (`sync-cli`)**: A utility for synchronizing API keys *from* one instance of The One Balance *to* current project instance. This is useful for maintaining multiple environments (e.g., staging, production) or for migrating instances. You can add multiply source and targets impl to import keys.

## Architecture

The system is designed to be highly available and resilient to failures, with a focus on intelligent key management and performance.

### Key Lifecycle, Load Balancing, and High Availability

The gateway's core architectural strength lies in its dynamic management of the API key pool, which provides resilience and distributes load across available keys.

1.  **Key Retrieval and Health Scoring**: When a request arrives, the system retrieves a list of healthy, `active` keys for the requested provider. These keys are sorted based on a health score that takes into account latency, success rate, and consecutive failures, ensuring the most reliable keys are tried first.
2.  **Failover Loop**: The system iterates through the sorted list, attempting the request with the healthiest key. If a **key fails** for any reason, the system automatically and transparently retries the request with the next key in the list.
3.  **Error Analysis and State Changes**: When a request fails, the system analyzes the error to determine the cause and takes immediate action:
    *   **Transient Errors**: If the error is a temporary server issue, the system will retry the request with the *same key*.
    *   **Key on Cooldown (e.g., Rate Limit)**: The key is immediately put on a temporary cooldown but still active, and the system moves on to the next key.
    *   **Invalid Key Errors**: The key is permanently marked as `blocked`, and the system moves on to the next key.
4.  **Two-Cache Design**: A two-level cache optimizes performance and resilience:
    *   A **Main Cache** holds the full list of healthy keys for each provider, updated periodically from the D1 database.
    *   A **Cooldown Cache** (or "Penalty Box") temporarily blacklists keys that have recently failed. This provides instant feedback to the failover loop, preventing it from retrying a key that is known to be on cooldown.
5.  **Cleanup Mechanisms**: The system has two ways to remove bad keys:
    *   **Automated Cleanup**: A scheduled background process periodically runs to perform live validation tests on `active` keys that have accumulated a high number of consecutive failures. If a key is confirmed to be permanently invalid during these tests, it is deleted from the system.
    *   **Manual Management**: The web UI allows for the manual deletion of any key, including those already marked as `blocked`.

### Advanced Timeout Mechanism

The system uses a multi-layered timeout strategy to ensure reliability and prevent requests from hanging:

1.  **Overall Request Timeout**: A top-level timeout (default: 25 seconds) wraps the entire request handling process. If this limit is exceeded, the request is aborted, and a `504 Gateway Timeout` is returned to the client.
2.  **Individual Attempt Timeout**: Each attempt to use a single API key has its own shorter timeout (default: 10 seconds).
3.  **Dynamic Timeout Calculation**: The system is adaptive. Before each attempt in the failover loop, it calculates the time remaining on the overall timeout. It then sets the timeout for the current attempt to be the *lesser* of the individual attempt timeout and the remaining time, ensuring it doesn't start an attempt it cannot finish.

### Technical Documentation

For more detailed technical explanations of the patterns used, please see the following documents in the `/docs` directory:

*   [`HYBRID_ORM_PATTERN.md`](./docs/HYBRID_ORM_PATTERN.md): Explains the custom ORM-like system for interacting with the D1 database.
*   [`two-cache-design.md`](./docs/two-cache-design.md): Describes the caching strategy for optimizing performance.

## Prerequisites

Before you begin, ensure you have the following installed:

*   [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
*   [Node.js](https://nodejs.org/en/) (v18 or later)
*   [pnpm](https://pnpm.io/installation)

## Getting Started

### Initial Setup

Before you can use the project, a one-time setup is required. All commands should be run from the workspace root.

First, ensure you have `just` installed:
```bash
cargo install just
```

1.  **Create Your Environment File**:
    Navigate into the worker directory, copy the example `.env` file, and fill in your secrets.
    ```bash
    cd crates/theone-balance
    cp .env.example .env
    ```

2.  **Configure Wrangler** in theone-balance dir:
    Set your non-secret variables in `wrangler.jsonc.tpl`.

3.  **Install Node.js Dependencies**:
    This command will correctly install `wrangler` and `drizzle-kit` inside the `crates/theone-balance` directory.
    ```bash
    cd ../..
    just install-all
    ```
4.  **Push Secrets to Cloudflare**:
    ```bash
    just secrets:push
    ```

After completing these steps, the entire workspace is set up. You can now use all other `just` commands (like `just dev`, `just sync`, etc.) from the root directory.

-   **For local development**: No other settings are needed.
-   **For production deployment**: You need to create an AI Gateway in the Cloudflare dashboard. This will give you a worker URL (e.g., `https://xx.xxx.worker.dev`) and an `AI_GATEWAY_TOKEN` which you need enable authenticated AI Gateway feature. You do not need to create a cloudflare worker manually; the project will do it for you based on the name in `wrangler.jsonc.tpl` you just set. Your Cloudflare Account ID can be found in the URL when you are logged into the dashboard.

## Project Usage

Here is an example of how to configure this gateway with `claude-code-route`:

```json
{
  "name": "cloudflare-ai-rust",
  "api_base_url": "https://xx.xxx.workers.dev/api/compat/chat/completions",
  "api_key": "AUTH_KEY",
  "models": ["google-ai-studio/gemini-2.5-pro", "google-ai-studio/gemini-2.5-flash"],
   "transformer": {
     "use": ["cloudflare-payload-fixer"]
   }
}

```
The cloudflare-payload-fixer transformer file located in crates/claude-code-router/transformers/payload-fixer.js,and it fix gemini return empty content issue, fix gemini tool-call cannot continue issue.


## Build and Deployment

This workspace uses a two-level system for managing builds and deployments, which provides both a simple top-level interface and a clear separation of concerns.

### 1. The Workspace Orchestrator (`justfile`)

The primary way to interact with the project is through the `justfile` located in the root of the workspace. It acts as a convenient command runner that delegates tasks to the appropriate crate.

You can list all available commands by running `just -l`. The primary commands are:

-   `just dev`: Starts the local development server for the Cloudflare Worker.
-   `just deploy`: Deploys the worker to your Cloudflare account.
-   `just migrate`: Runs database migrations against your local development database.
-   `just migrate-remote`: Runs database migrations against your production Cloudflare D1 database.
-   `just secrets-push`: Securely pushes secrets from your `.env` file to your Cloudflare worker's environment.
-   `just build-cli`: Compiles the `sync-cli` utility binary.
-   `just sync`: Runs the `sync-cli` to synchronize data between instances (builds the CLI first).

### 2. The Core Worker Logic (`crates/theone-balance`)

The actual build, deployment, and configuration logic for the Cloudflare Worker resides within the `crates/theone-balance` directory. This is where `wrangler`, `pnpm`, and `drizzle-kit` are configured. The root `justfile` simply calls the `pnpm` scripts defined here.

This project uses `pnpm` to orchestrate `wrangler` and `drizzle-kit` for managing the Cloudflare Worker environment. All commands should be run from within the `crates/theone-balance` directory.

*   **`pnpm dev`**: Starts the local development server for the Cloudflare Worker. It uses `wrangler` to build the Rust `cdylib` and serve it locally.
*   **`pnpm migrate`**: Runs database migrations using `drizzle-kit`. This is used to keep the D1 database schema up to date.
*   **`pnpm deploy`**: Deploys the Cloudflare Worker to the Cloudflare network. This script builds the Rust `cdylib` in release mode, bundles it with Wrangler, and pushes it to your Cloudflare account.

## Feature Flags

The `crates/theone-balance/Cargo.toml` file defines several feature flags to control which Rust binary is compiled:

*   **`default`**: Compiles the Cloudflare Worker library (`cdylib`).
*   **`sync_cli`**: Compiles the `sync-cli` binary for synchronizing keys between instances.

## Testing

### Current Testing Strategy

The project employs a multi-faceted testing strategy tailored to the needs of each crate within the workspace:

**1. `theone-balance` (The Core Worker):**
   - **Method**: The worker is tested via an integration test located in `tests/integration_test.rs`.
   - **Strategy**: The current strategy focuses on testing the core business logic, such as the health-based key selection and circuit breaker pattern. It uses helper functions in `src/testing.rs` to validate live keys against provider endpoints.
   - **Limitation**: The primary integration test is currently ignored (`#[ignore]`) because it requires a live connection to a D1 database, preventing it from running in a fully automated CI environment.

### Testing To-Do List & Roadmap

To improve test coverage and reliability, the following tasks are planned:

- [ ] **Decouple Tests from Live Services**: Refactor the main integration test to use a mock or in-memory database (e.g., in-memory SQLite) instead of requiring a live D1 connection. This will allow the core business logic tests to be un-ignored and run automatically.
- [ ] **Implement HTTP API Endpoint Tests**: Add a new suite of tests using `axum-test` to validate the behavior, status codes, and responses of all API endpoints defined in the Axum router.
- [ ] **Add a Dedicated Test Suite for the CLI**: Create a new test file (`tests/cli_test.rs`) that uses `assert_cmd` to test the `sync-cli` binary, covering its arguments, output, and exit codes.
- [ ] **Expand `theone-balance` Test Coverage**: Following the example set by the `toasty` crate, progressively add more tests to `theone-balance` to cover failover logic, retry mechanisms, and metric updates.

## Roadmap

- [ ] **Local Double Build**: Fix double build when run just dev 
- [ ] **Other Provider Test and Validation**: Add support for and validate other AI providers.
- [ ] **Reduce Binary Size**: Optimize the final binary to reduce its footprint.
- [ ] **Support Raw Socket to Native Endpoints**: Implement direct raw socket connections to native provider endpoints for lower latency.
- [ ] **Add Transformers for `claude-code-route`**: Implement request/response transformers to address provider-specific issues, such as fixing the Gemini stop sequence problem.
- [ ] **Sync Keys from Other Sources**: Expand the `sync-cli` to support importing keys from various sources.
- [ ] **Cross-Provider Load Balancing**: Implement intelligent, prompt-based load balancing across different AI providers.
- [ ] **Refine Timeout Settings**: Fine-tune the timeout mechanism for better performance and reliability.
- [ ] **HA to other cloud provider****: XXX. 
- [ ] **Chinese Support****: XXX. 



### Ad-hoc Tests

#### Local Server Test
```bash
# OpenAI-Compatible chat
curl http://localhost:8087/api/compat/chat/completions \
 -H "Content-Type: application/json" -H "cf-aig-authorization: Bearer local-cf-api-token" \
 -H "Authorization: Bearer local-auth-key" \
 -d '{
   "model": "google-ai-studio/gemini-2.5-pro",
   "messages": [
     {
       "role": "user",
       "content": "你好！"
     }
   ]
 }'

# OpenAI-Compatible embeddings
curl "http://localhost:8087/api/compat/embeddings" -H "Content-Type: application/json" -H "Authorization: Bearer local-auth-key" -H "cf-aig-authorization: Bearer locl-cf-api-token" -d '{"input": "This is a test sentence for embeddings.", "model": "google-ai-studio/text-embedding-004"}'

# Provider-specific Gemini format
curl -X POST "http://localhost:8087/api/google-ai-studio/v1beta/models/text-embedding-004:batchEmbedContents" \
 -H "Content-Type: application/json" \
 -H "cf-aig-authorization: Bearer local-cf-api-token" \
 -H "Authorization: Bearer local-auth-key" \
 -d '{"requests": [{"model": "models/text-embedding-004", "content": { "parts": [ { "text": "This is a test for the native Gemini API." } ] }}] }'

# Test clean active keys but consecutive_failures > 50 * RECOVERY_THRESHOLD
curl -X POST -H "Authorization: Bearer local-auth-key" http://localhost:8087/test/run-cleanup/google-ai-studio

# Add keys to local db
curl -X POST http://localhost:8087/api/keys/add/google-ai-studio -H "Authorization: Bearer local-auth-key" -H "Content-Type: text/plain" --data "keys=AIzaSyDodA02wD3MtRB_b0zBjrvhLrGtcF_RcJc,AIzaSyBhjuhAKCq8cST3S8JdUbdyCJTI_c1y6GE,AIzaSyByoMeX_ayIbfEE00pHq7f9H4PJE0AAWQv"
```

#### Worker Test
```bash
# Provider-specific Gemini format
curl -X POST "https://xx.xxx.workers.dev/api/google-ai-studio/v1beta/models/text-embedding-004:batchEmbedContents" -H "Content-Type: application/json" -H "cf-aig-authorization: Bearer GRT1Y_kDjWuieTEH-JTYm-u8hPuKEqiDS8t784WR" -H "x-goog-api-key: AUTH_KEYvalue" -d '{"requests": [{"model": "models/text-embedding-004", "content": { "parts": [ { "text": "This is a test for the native Gemini API." } ] }}] }'

# OpenAI-Compatible embeddings
curl "https://xx.xxx.workers.dev/api/compat/embeddings" -H "Content-Type: application/json" -H "Authorization: Bearer AUTH_KEYvalue" -H "cf-aig-authorization: Bearer GRT1Y_kDjWuieTEH-JTYm-u8hPuKEqiDS8t784WR" -d '{"input": "This is a test sentence for embeddings.", "model": "google-ai-studio/text-embedding-004"}'

# OpenAI-Compatible chat
curl "https://xx.xxx.workers.dev/api/compat/chat/completions" -H "Content-Type: application/json" -H "Authorization: Bearer AUTH_KEYvalue" -H "cf-aig-authorization: Bearer GRT1Y_kDjWuieTEH-JTYm-u8hPuKEqiDS8t784WR" -d '{    "model": "google-ai-studio/gemini-2.5-pro",   "messages": [      {       "role": "user",        "content": "Hello!"      }   ]  }'
```

## Debuging
### Remote worker log:
* In the `crates/theone-balance` dir, run `npx wrangler tail`
* Or, run `just tail`
* Or, login to the Cloudflare dashboard and view the worker logs.

### Local worker log
* The `just dev` console will show local logs.


## Acknowledgements

This project was inspired by and references the excellent work done on `one-balance`. We are grateful to its contributors for their ideas and the foundation they provided.
You can find the original project here: https://github.com/glidea/one-balance


## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.






