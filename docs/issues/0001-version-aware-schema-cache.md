# Skip schema refetches when the server version has not changed

Status: open (not scheduled)
Depends on: phase 1 (search and navigate) being merged

## Problem

The schema cache only knows its age. After 24 hours it is flagged stale (red status bar, stderr warning) even if the server has not changed, and the user has to refresh by hand to be sure. The full introspection query is expensive on a large schema.

Some servers expose a cheap way to tell that they changed, for example a version string reachable with a tiny query. When that is available, vega can use it to decide whether the cached schema is still valid, and fetch the full schema only when the version differs.

Vega is a general GraphQL client, so this must be opt-in per server and must never assume any particular server.

## Hard requirement

When no version query is configured, or the probe fails, vega behaves exactly as it does today (age-based staleness, manual refresh). Every new behavior below is additive.

## Proposed configuration

Two optional settings, in `config.toml` now and per profile once profiles exist:

```toml
version_query = "{ info { server { serverVersion } } }"   # any query returning a scalar
version_path  = "info.server.serverVersion"               # dotted path into `data`
```

- The probe is a normal GraphQL `POST` to the same endpoint with the same `X-API-Key` header, so it needs no extra credentials.
- The value found at `version_path` is compared as a string (numbers and booleans are stringified). A missing or null value counts as a failed probe.
- Open choice: environment variable and flag equivalents (`VEGA_VERSION_QUERY`, `--version-query`) versus config file only. Config file only is enough to start; the query text is awkward in a shell.

## Cache changes

Add to the cache entry, both optional so existing cache files keep loading (`#[serde(default)]`):

- `server_version: Option<String>`: the version observed when the schema was fetched.
- `verified_at: Option<u64>`: unix seconds of the last probe that matched the stored version.

## Behavior sketch

On startup, when a version query is configured and a cache exists:

1. Send the probe (short timeout, a few seconds).
2. Same version: the cache is considered current. Update `verified_at`, and do not show the stale warning or the red bar.
3. Different version: the schema is outdated. Either refetch automatically or mark it outdated and let the user refresh (see open questions).
4. Probe fails (network, GraphQL error, missing value): fall back to the age-based rules and say so in a one-line note (stderr for the CLI, status bar for the TUI).

With no cache: fetch the schema as today, then probe once and store `server_version` with it.

## Status bar and CLI wording

Keep the two ages separate so the bar stays honest:

- version verified: `host · schema v1.2.3 verified 2m ago (fetched 3d ago)`, neutral colour
- not verifiable: today's `schema fetched 3d ago`, red after 24 hours

## Open questions (decide while brainstorming)

1. Automatic refetch on a version change, or only flag it ("server changed, Ctrl-R to refresh")? Automatic is convenient but can make a `--search` call unexpectedly slow and noisy for scripts.
2. CLI latency: the probe adds a round trip to every `--search` and `--describe`. Options: probe only when the cache is older than some age; probe always with a short timeout; add `--no-version-check`.
3. TUI: run the probe in the background after the first frame (never block startup), and update the bar when it returns.
4. A version string may not change when the schema does (development builds, hot fixes). Keep manual refresh, and consider a configurable ceiling (`max_age`) that forces a refetch even when the version matches.
5. Which part of a version change counts? Any difference, or only a configurable pattern? Any difference is the simple default.
6. Per-server settings should live with profiles; decide whether this issue waits for profiles or ships with a single global setting first.

## Acceptance criteria

- With no `version_query`, behavior and output are byte-for-byte unchanged (existing tests still pass).
- With a version query and an unchanged version, a `--search` run sends one small request, no introspection, exits 0 and prints no stale warning even if the cache is older than 24 hours.
- With a changed version, the outcome follows the decision for open question 1, and the cache keeps its previous content if the refetch fails.
- A failing probe never breaks a command that works from the cache today.
- The API key never appears in the cache, logs or error messages (same rule as the rest of vega); server error bodies from the probe are truncated and redacted like introspection errors.
- Tests use a local `127.0.0.1` listener only: unchanged version, changed version, probe error, missing value, old cache file without the new fields.

## Out of scope

- Schema diffing or change notes between versions.
- Detecting changes by hashing the introspection result (a separate, heavier idea).
- Push notifications or polling while the TUI is open.
