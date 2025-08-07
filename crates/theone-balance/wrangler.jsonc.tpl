{
    //This wrangler is for worker delopy, so the config below is for worker. 
    "$schema": "https://developers.cloudflare.com/workers/wrangler/wrangler-schema.json",
    // worker name, we keep aigateway-name same as the worker name. so when
    // deploy, must set AI_GATEWAY=test. here. But in fact, the name can be
    // diffrent.
    "name": "test",
    "main": "build/worker/shim.mjs",
    "compatibility_date": "2025-07-21",
    "build": {
        // "command": "RUST_BACKTRACE=1 cargo install -q worker-build && worker-build --release"
        "command": "cargo install -q worker-build && worker-build --release"
       // "command": "cargo install -q worker-build && worker-build --release",
       // "watch_dir": ["src"]
    },
    "ai": {
        "binding": "AI"
    },
    "d1_databases": [
        {
            "binding": "DB",
            "database_name": "llmtest",
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
        //AI bind: Ai gateway name 
        "AI_GATEWAY": "aigateway-name",
        "CLOUDFLARE_API_TOKEN": "xxxxx",
        "CLOUDFLARE_ACCOUNT_ID": "xxxx",
        "AI_GATEWAY_TOKEN": "xxxx",
        "IS_LOCAL": "false",
        // "RUST_LOG": "warn,one_balance_rust::handlers=info"
        "RUST_LOG": "warn",
        // "OVERALL_TIMEOUT_MS": "240000", 
        "OVERALL_TIMEOUT_MS": "40000", 
       // default 25
       // "TARGET_TIMEOUT_MS": "40000"
        "TARGET_TIMEOUT_MS": "10000"
       // default 10
    },
    "observability": {
      "enabled": true,
      "head_sampling_rate": 1
    }
//    "env": {
//      "dev": {
//        "build": {
//          "watch_ignore": ["wrangler.jsonc", "./migrations/schema.sql"]
//        }
//      }
//    }

}
