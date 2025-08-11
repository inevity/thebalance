# Orchestration commands for The One workspace.

# --- Worker Commands ---

# Install all Node.js dependencies for the worker crate.
install-all:
    cd crates/theone-balance && pnpm install

# Run the local development server for the worker.
dev:
    cd crates/theone-balance && pnpm run dev

# Deploy the worker to the configured Cloudflare account.
deploy:
    cd crates/theone-balance && pnpm run deploy

# Run database migrations against the local D1 database.
migrate:
    cd crates/theone-balance && pnpm run migrate

# Run database migrations against the remote production D1 database.
migrate-remote:
    cd crates/theone-balance && pnpm run migrate:remote

# Securely upload secrets from the .env file to Cloudflare.
secrets-push:
    cd crates/theone-balance && pnpm run secrets:push

tail:
    cd crates/theone-balance && npx wrangler tail

# --- CLI Commands ---

# Build the sync-cli binary.
build-cli:
    cargo build --no-default-features --bin sync-cli --features sync_cli

# Run the sync process between instances using default configuration.
# This command automatically builds the CLI first.
sync: build-cli
    ./target/debug/sync-cli sync --source one-balance --source-name one-balance --target the-one
