# Brainstorm: other forms of login

Status: open (needs brainstorming)
Related: 0004 (paste API key), 0007 (research on authentication methods)

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
