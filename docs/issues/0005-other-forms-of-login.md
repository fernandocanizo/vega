# Brainstorm: other forms of login

Status: done
Related: 0004 (paste API key), 0007 (research on authentication methods)
Resolved by: 0004 (design in `docs/superpowers/specs/2026-10-24-connection-modal-design.md`)

## Problem

Vega authenticates with an API key sent as the `X-API-Key` header. Many GraphQL servers use something else: bearer tokens, OAuth flows, session cookies, custom headers. Vega is a general GraphQL client, so it should be able to talk to them.

## Questions to settle

- Which methods are worth supporting, based on the findings of 0007?
- Configuration: a configurable header name and scheme (for example `Authorization: Bearer ...`) versus dedicated login flows.
- Interactive flows (OAuth device code, browser redirect) in a terminal tool: worth the complexity?
- Where secrets and tokens live. The existing rules forbid persisting the API key and variable values; decide what applies to tokens.
- Interaction with profiles (0003).

## Deliverable

A recommendation with tradeoffs, and implementation issues split from it. No code in this issue.

## Resolution

Decided and delivered together with 0004. Details are in `docs/authentication.md`.

- Methods: None, Basic, API Key, Bearer and Custom headers. All of them are ways to produce HTTP headers, so any server that authenticates through headers works.
- Configuration: a configurable header name (`--key-header`, `VEGA_GRAPHQL_KEY_HEADER`) plus free-form headers (`VEGA_GRAPHQL_HEADERS`), instead of one flow per scheme. The same forms exist in the connection modal (`Ctrl-K`).
- Secrets: header values come only from the environment or the modal, live in memory only, and are masked in everything shown. No secret flag exists.
- Profiles (0003): not decided here. The modal returns a `Settings` value, which is the natural unit for one profile.

Not covered, and not scheduled: OAuth 2.0 (device code, authorization code with PKCE, client credentials), session cookies and mutual TLS. They need interactive flows or other transports and deserve their own issues.
