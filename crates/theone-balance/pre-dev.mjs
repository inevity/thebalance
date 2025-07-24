// one-balance-rust/pre-dev.mjs
import * as fs from 'fs';

function getWranglerConfig() {
    const configStr = fs.readFileSync('wrangler.jsonc', 'utf-8');
    // wrangler.jsonc is JSON with Comments. We need to strip them before parsing.
    const jsonStr = configStr.replace(/\/\*[\s\S]*?\*\/|\/\/.*/g, '');
    try {
        return JSON.parse(jsonStr);
    } catch (e) {
        console.error("Failed to parse wrangler.jsonc. It might be invalid JSONC.", e);
        // Fallback for simple cases where it might be valid JSON already
        return JSON.parse(fs.readFileSync('wrangler.jsonc', 'utf-8'));
    }
}

function main() {
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

    console.log('Writing updated configuration to wrangler.jsonc...');
    fs.writeFileSync('wrangler.jsonc', JSON.stringify(config, null, 4));
    console.log('Configuration ready for local development.');
}

main();
