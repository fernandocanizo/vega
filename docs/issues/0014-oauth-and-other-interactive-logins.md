# Support OAuth 2.0 and other logins that are not plain headers

Status: open (not scheduled)
Related: 0005 (other forms of login), 0007 (closed as wontdo), 0003 (multiple profiles)

## Problem

Vega sends credentials the user already has: an API key, a bearer token, basic authentication or any custom headers (see `docs/authentication.md`). That covers servers whose credentials do not expire or that the user can renew by hand. Some servers need a login flow first, and Vega cannot talk to them today:

- OAuth 2.0, with the grants Altair offers: authorization code, authorization code with PKCE and client credentials.
- Session cookies set by a login page.
- Mutual TLS (a client certificate).

## When to do it

Not scheduled. Altair's methods were judged enough while Vega has a single user. Pick this up when someone needs one of these methods, and start with that one only.

## Questions to settle

- Which flow first, and does it need a browser? A terminal tool can open the browser and listen on a local redirect URI, or use the device code flow, which needs no redirect.
- Client credentials is the simplest (no user interaction): a token request with a client ID and secret. Is it enough for a first step?
- Where tokens live. The rules say header values stay in memory only and are masked everywhere. Access and refresh tokens should follow them unless a persistent store is decided explicitly.
- Refreshing an expired token during a session without losing the user's query and variables.
- How it fits the connection modal: a new `AuthType` unit in `tui/connection.rs` that produces headers, with its own fields (token URL, client ID, client secret, scope) and a "get token" step.
- Configuration for scripts and agents, where there is no UI: environment variables only, like the other secrets.
- Interaction with profiles (0003): one set of credentials per profile.

## Deliverable

A recommendation with tradeoffs for the first method, then implementation issues. No code in this issue.
