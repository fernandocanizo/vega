# Send a User-Agent header so GitHub's GraphQL API can be used

Status: done
Related: 0006 (find public GraphQL servers)

## Problem

Vega sends no `User-Agent` header. `reqwest` does not add one, and nothing in `vega-core` sets it. Verified with a local listener: a request carries only `content-type`, `content-length`, `accept: */*` and `host`.

GitHub's GraphQL API rejects such requests before it looks at any token:

```
$ vega --uri https://api.github.com/graphql --search ""
Error: fetching the schema from https://api.github.com/graphql

Caused by:
    server answered HTTP 403:
    Request forbidden by administrative rules. Please make sure your request has a User-Agent header (...)
```

With any `User-Agent`, `curl` gets past that check and reaches the normal unauthenticated answer. So Vega cannot talk to api.github.com at all, with or without a key. Found while checking servers for 0006.

## Expected behavior

Every request carries `User-Agent: vega/<version>` (the crate version, no OS or user data). The header is not a secret and is not configurable in this issue.

## Out of scope

- Letting the user override the User-Agent.

## Resolution

Every request sends `User-Agent: vega/<version>`: the product token and the version, the form RFC 9110 section 10.1.5 describes, with nothing about the system or the user. A browser string was not used: the RFC discourages claiming to be another implementation. A `User-Agent` header given in `VEGA_GRAPHQL_HEADERS` or the connection modal replaces it, so the override that was out of scope here already works.
