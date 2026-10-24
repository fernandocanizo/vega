# Find public GraphQL servers to test Vega against

Status: open (not scheduled)

## Problem

Vega is only exercised against the shared introspection fixture and local listeners. Real servers differ in schema size, naming, auth, introspection support and error shapes, and would show problems the fixture cannot.

## Task

Search for public GraphQL servers that can be used without credentials, or with a free key, and list them with:

- the endpoint URL;
- whether introspection is enabled;
- the authentication it needs, if any;
- the approximate schema size and anything notable (subscriptions, deprecated fields, custom scalars, interfaces and unions);
- the terms of use or rate limits.

Record the result in a document under `docs/`. Check each endpoint with vega before listing it.

## Acceptance criteria

- At least a handful of servers of different sizes and styles are listed, each verified with vega.
- Any bug found along the way is filed as its own issue.

## Out of scope

- Automated tests against real servers. Tests never use the real network.
