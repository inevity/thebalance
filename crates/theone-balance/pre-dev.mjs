import { execSync } from 'child_process';
import * as fs from 'fs';
import { parse } from 'jsonc-parser';

function run(command, options = {}) {
    console.log(`> ${command}`);
    execSync(command, { stdio: 'inherit', ...options });
}

function exec(command, options = {}) {
    console.log(`> ${command}`);
    return execSync(command, { encoding: 'utf-8', ...options });
}

function extractValidJson(output) {
    const arrMatch = output.match(/\[[\s\S]*?\]/);
    if (arrMatch) {
        return JSON.parse(arrMatch[0]);
    }
    const objMatch = output.match(/\{[\s\S]*\}/);
    if (objMatch) {
        return JSON.parse(objMatch[0]);
    }
}

function json(command, options = {}) {
    return extractValidJson(exec(command, options));
}

function getWranglerConfig() {
    const configStr = fs.readFileSync('wrangler.jsonc', 'utf-8');
    try {
        return parse(configStr);
    } catch (e) {
        console.error("Failed to parse wrangler.jsonc.", e);
        process.exit(1);
    }
}

async function main() {
    console.log('Preparing configuration for local development...');
    const config = getWranglerConfig();

    const authKey = process.env.AUTH_KEY;
    const aiGatewayToken = process.env.AI_GATEWAY_TOKEN;

    if (!config.vars) {
        config.vars = {};
    }

    if (authKey) {
        console.log("Setting AUTH_KEY from environment variable.");
        config.vars.AUTH_KEY = authKey;
    } else {
        console.log("AUTH_KEY not set, using default for local dev.");
        config.vars.AUTH_KEY = "local-auth-key";
    }

    if (aiGatewayToken) {
        console.log("Setting AI_GATEWAY_TOKEN from environment variable.");
        config.vars.AI_GATEWAY_TOKEN = aiGatewayToken;
    } else {
        console.log("AI_GATEWAY_TOKEN not set, using default for local dev.");
        config.vars.AI_GATEWAY_TOKEN = "local-ai-gateway-token";
    }

    console.log('Checking for D1 databases...');
    let dbs = json('wrangler d1 list --json');
    const existingDBNames = new Set(dbs.map(db => db.name));

    for (const db of config.d1_databases) {
        if (!existingDBNames.has(db.database_name)) {
            console.log(`Creating D1 database '${db.database_name}'...`);
            run(`wrangler d1 create ${db.database_name}`);
        } else {
            console.log(`D1 database '${db.database_name}' already exists.`);
        }
    }

    console.log('Refreshing D1 database list to sync IDs...');
    dbs = json('wrangler d1 list --json');
    const dbNameToId = new Map(dbs.map(db => [db.name, db.uuid]));

    for (const dbConfig of config.d1_databases) {
        const dbId = dbNameToId.get(dbConfig.database_name);
        if (dbId) {
            dbConfig.database_id = dbId;
        }
    }

    console.log('Writing updated configuration to wrangler.jsonc...');
    fs.writeFileSync('wrangler.jsonc', JSON.stringify(config, null, 4));
    console.log('Configuration ready for local development.');
}

main();
