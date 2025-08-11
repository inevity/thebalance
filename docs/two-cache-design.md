  Final Consolidated Design Plan: The Two-Cache Solution

  Objective: To solve the performance issue where multiple failovers occur for a single user request. This is caused by the
  current system's inability to share real-time key failure information with the currently executing request, which operates
  on a stale list of keys. This new design provides instant feedback while maintaining a robust and correct caching strategy.

  Part 1: Core Cache Architecture (src/d1_storage.rs)

  We will define two distinct, static caches to separate the concerns of long-term health from short-term availability.

  1. API_KEY_CACHE (The Main List Cache):
    - Purpose: To store the main, sorted list of all known healthy API keys for a given provider. This list is expensive to
  generate, requiring a database query and sorting logic.
    - Implementation: This cache already exists. Its current definition, using a 60-second fixed Time-To-Live (TTL), will
  remain unchanged.
  2. COOLDOWN_CACHE (The "Penalty Box" Cache):
    - Purpose: To serve as a high-speed, temporary blacklist for keys that have just failed a request. It provides the
  instant, in-memory feedback needed to prevent the current failover loop from retrying a key that is known to be unavailable.
    - Implementation (Revised): We will add a new static cache with the following precise characteristics:
        - Key: The ApiKey ID (String).
      - Value: The unit type (), as only the key's presence matters.
      - TTL: This cache will have no default TTL. The TTL will be set dynamically for each entry upon insertion, allowing us
  to respect the exact cooldown duration specified by an API provider.
      - Max Capacity: A high capacity (e.g., 10,000) to handle many failing keys across all providers.

  Part 2: Key Selection Logic (src/d1_storage.rs)

  The function get_healthy_sorted_keys_via_cache will be updated to use both caches to produce the final, usable list of keys.

  1. The function will first attempt to fetch the list of keys from the main API_KEY_CACHE. If it's a miss, it will fetch the
  full list from the D1 database and populate the cache. This behavior is unchanged.
  2. New Filtering Step: Before returning the list, it will perform an in-memory filtering operation. It will iterate through
  the list fetched from the API_KEY_CACHE and remove any key whose ID is currently present in the COOLDOWN_CACHE.
  3. Result: This function will now return a list of keys that are not only considered healthy in the long-term (active, not
  circuit-broken) but are also not on a temporary, immediate cooldown.

  Part 3: Handling State Changes (The Core Logic)

  This section details how different events are handled by the new system.

  1. New Helper Function for Dynamic Cooldowns (src/d1_storage.rs):
    - To centralize the logic for adding keys to the penalty box, we will create one new function:
    pub fn flag_key_with_cooldown(key_id: &str, duration_seconds: u64) {
      info!(key_id, duration_seconds, "Flagging key for temporary cooldown in local cache.");
      // Uses the cache's ability to set TTL per entry.
      COOLDOWN_CACHE.insert_with_ttl(key_id.to_string(), (), Duration::from_secs(duration_seconds));
  }
  2. Handling Different Failure Types (src/handlers.rs):

    - Case A: Temporary Cooldown (e.g., a 429 Rate Limit error)
        - Context: Occurs in src/handlers.rs inside the KeyOnCooldown error handling block.
      - Action 1 (Immediate): We will parse the cooldown_seconds from the provider's error message and call
  d1_storage::flag_key_with_cooldown(&key_id, cooldown_seconds). This respects the provider's requested cooldown time
  precisely.
      - Action 2 (Background): The existing logic to update the key's cooldown time in the D1 database will remain for
  persistence.
      - The main API_KEY_CACHE will NOT be invalidated, as this is a temporary state change.
    - Case B: Permanent Block (e.g., a 401 Invalid Key error)
        - Context: Occurs in src/handlers.rs inside the KeyIsInvalid error handling block.
      - Action 1 (Immediate): We will call d1_storage::flag_key_with_cooldown(&key_id, 300). We use a fixed, long duration
  (300 seconds) as an immediate safety net to ensure the key is ignored by the current request's failover loop.
      - Action 2 (Background): The existing logic to call d1_storage::update_status to permanently set the key to blocked in
  the D1 database will remain.
      - Action 3 (Cache Purge): The update_status function itself will be modified to include a call to
  API_KEY_CACHE.invalidate(&provider). This ensures the next request gets a fresh list from D1, which will now correctly
  exclude the permanently blocked key.
    - Case C: Administrative Changes (Add/Delete Keys)
        - Context: Occurs in d1_storage.rs within the functions add_keys, delete_keys, and delete_all_blocked.
      - Action: Each of these functions will be modified to ensure they explicitly call API_KEY_CACHE.invalidate(&provider)
  after a successful database modification.

  Part 4: Code Cleanup

  The flawed function d1_storage::update_key_in_cache and all calls to it will be deleted entirely. It is now obsolete, and
  its responsibilities are handled correctly and more efficiently by the new design.


