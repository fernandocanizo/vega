# Research: authentication methods common in GraphQL servers

Status: open (not scheduled)
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
