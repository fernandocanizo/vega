# Research: authentication methods common in GraphQL servers

Status: wontdo (Vega copies the methods Altair offers)
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

## Resolution

Won't do, for now. The research was meant to decide which login methods Vega should support. That was settled in 0005 by following Altair, the established GraphQL client: None, API Key, Basic, Bearer and Custom headers are implemented, and OAuth 2.0 is tracked in 0014. Altair's list and its sources are in the "Reference: Altair" section of `docs/superpowers/specs/2026-10-24-connection-modal-design.md`.

With a single user and the current methods covering real use, surveying every server and framework would not change what gets built. If other people start using Vega and ask for a method that is missing, open an issue for that method and research it then.
