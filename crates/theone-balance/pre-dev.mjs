import { execSync } from 'child_process';
import * as fs from 'fs';
import { parse } from 'jsonc-parser';

// --- Helper functions directly from pre-deploy.mjs ---

function run(command, options = {}) {
    console.log(`> ${command}`);
    try {
        execSync(command, { stdio: 'inherit', ...options });
    } catch (e) {
        // For local dev, it's often fine if commands fail (e.g., creating an existing DB).
        console.warn(`Command "${command}" failed. This might be expected during local setup and can often be ignored.`);
    }
}

function commandExists(command) {
    const checkCmd = process.platform === 'win32' ? 'where' : 'command -v';
    try {
        execSync(`${checkCmd} ${command}`, { stdio: 'ignore' });
        return true;
    } catch (e) {
        return false;
    }
}

// --- getWranglerConfig adapted to use jsonc-parser ---

function getWranglerConfig() {
    const configStr = fs.readFileSync('wrangler.jsonc', 'utf-8');
    try {
        return parse(configStr);
    } catch (e) {
        console.error("Failed to parse wrangler.jsonc. Please ensure it is a valid JSONC file.", e);
        process.exit(1);
    }
}

// --- Main logic adapted for LOCAL development ---

async function main() {
    console.log('Preparing configuration for local development...');

    if (!commandExists('wrangler')) {
        console.error('Wrangler is not installed. Please install it by running: pnpm install wrangler');
        process.exit(1);
    }
    
    const config = getWranglerConfig();

    // Set default local vars if not present.
    if (!config.vars) { config.vars = {}; }
    const varsToSet = {
        AUTH_KEY: process.env.AUTH_KEY || "local-auth-key",
        AI_GATEWAY: process.env.AI_GATEWAY || "one-balance",
        CLOUDFLARE_ACCOUNT_ID: process.env.CLOUDFLARE_ACCOUNT_ID || "local_account_id",
        CLOUDFLARE_API_TOKEN: process.env.CLOUDFLARE_API_TOKEN || "local-cf-api-token",
        IS_LOCAL: "true"
    };

    console.log('Setting/overwriting local development variables...');
    for (const [key, value] of Object.entries(varsToSet)) {
        config.vars[key] = value;
    }

 //   // Ensure local D1 databases exist.
 //   if (config.d1_databases && config.d1_databases.length > 0) {
 //       console.log('Checking for local D1 databases...');
 //       for (const db of config.d1_databases) {
 //           console.log(`Ensuring local D1 database '${db.database_name}' exists...`);
 //           // This command is idempotent for local databases.
 //           run(`wrangler d1 create ${db.database_name}`);
 //       }
 //   } else {
 //       console.warn("No d1_databases found in wrangler.jsonc to check.");
 //   }
    
    // Write the final configuration file.
    console.log('Writing updated configuration to wrangler.jsonc...');
    fs.writeFileSync('wrangler.jsonc', JSON.stringify(config, null, 4));
    console.log('Local configuration is ready.');
}

// Correctly await the async main function.
await main();
