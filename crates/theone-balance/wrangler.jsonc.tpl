{
    "$schema": "https://developers.cloudflare.com/workers/wrangler/wrangler-schema.json",
    "name": "theone",
    "main": "build/worker/shim.mjs",
    "compatibility_date": "2025-07-21",
    "build": {
        // "command": "RUST_BACKTRACE=1 cargo install -q worker-build && worker-build --release"
        "command": "cargo install -q worker-build && worker-build --release"
    },
    "ai": {
        "binding": "AI"
    },
    "d1_databases": [
        {
            "binding": "DB",
            "database_name": "llm",
            "database_id": "xfefef",
            "migrations_dir": "migrations"
        }
    ],
//    "queues": {
//        "producers": [
//            {
//                "queue": "state-updater",
//                "binding": "STATE_UPDATER"
//            }
//        ],
//        "consumers": [
//            {
//                "queue": "state-updater"
//            }
//        ]
//    },
    "vars": {
        "AUTH_KEY": "my-auth-key",
        "AI_GATEWAY": "aigateway-name",
        "CLOUDFLARE_API_TOKEN": "xxxxx",
        "CLOUDFLARE_ACCOUNT_ID": "xxxx",
        "AI_GATEWAY_TOKEN": "xxxx",
        "IS_LOCAL": "false"

    },
    "observability": {
      "enabled": true,
      "head_sampling_rate": 1
    }

}
