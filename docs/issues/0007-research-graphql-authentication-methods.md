# Research: authentication methods common in GraphQL servers

Status: open (partly covered by 0004)
Feeds: 0004 (paste API key), 0005 (other forms of login)

## Problem

Before deciding which login methods vega should support, we need to know what GraphQL servers actually use.

## Task

Research, using primary sources (server and framework documentation, specifications), the usual authentication methods: API keys in headers or query strings, bearer and JWT tokens, OAuth 2.0 flows, session cookies, basic auth, mutual TLS, custom headers, and how introspection access is gated.

For each method, note how a client sends credentials, how common it is, and what it would mean for a terminal client. Attribute claims to their source URLs.

## Deliverable

A Markdown file under `docs/` with the findings and a short recommendation of what vega should support first.

## Out of scope

- Implementation.

## Progress

Done as part of 0004:

- Altair's authorization types (None, API Key, Basic, Bearer, OAuth 2.0 with authorization code, PKCE and client credentials), with sources, are in the "Reference: Altair" section of `docs/superpowers/specs/2026-10-24-connection-modal-design.md`.
- The header-based methods are implemented (see 0005).

Still missing, so this issue stays open: the Markdown file under `docs/` that surveys what GraphQL servers and frameworks use (API keys in query strings, JWT, session cookies, mutual TLS, how introspection access is gated, and so on), with sources and a recommendation. That research is what decides whether OAuth 2.0 and the other interactive flows are worth building.
