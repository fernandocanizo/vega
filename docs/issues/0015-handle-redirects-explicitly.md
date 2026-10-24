# Handle redirects explicitly: say what happened and never send credentials to another origin

Status: open (not scheduled)
Related: 0006 (find public GraphQL servers), 0010 (clear errors)

## Problem

Vega uses the default redirect policy of its HTTP client (`reqwest` 0.13.5: follow up to 10 redirects). Two things follow from that.

### The error hides the cause

For a 301, 302 or 303 the client repeats the request as a `GET` without the body. A GraphQL server then complains about a missing query and the real cause, a redirect, is never shown. Found while checking public servers for 0006:

```
$ vega --uri https://swapi-graphql.netlify.app/.netlify/functions/index --search planet
Error: fetching the schema from https://swapi-graphql.netlify.app/.netlify/functions/index

Caused by:
    server answered HTTP 400: {"errors":[{"message":"Missing query"}]}
```

That URL answers a `POST` with `301` and `Location: https://swapi-graphql.netlify.app/graphql`, which is the working endpoint. `curl -L` fails in the same way.

### Credentials are sent to the redirect target

When a redirect leaves the origin (another host, port or scheme), the client removes only `Authorization`, `Cookie`, `Cookie2`, `Proxy-Authorization` and `WWW-Authenticate` (`remove_sensitive_headers` in `reqwest/src/redirect.rs`). Every other header is kept, and Vega's credentials are normally other headers: the default `X-API-Key`, any name given with `--key-header`, and everything from `VEGA_GRAPHQL_HEADERS` or the connection modal.

Verified with two local listeners on 127.0.0.1: the first answers `307` with `Location: http://127.0.0.1:<other port>/`, and `VEGA_GRAPHQL_KEY=k1-secret vega --uri http://127.0.0.1:<first port>/graphql --search x` made the second listener receive `POST` with `x-api-key: k1-secret`. A misconfigured or hostile server (or a typo in the URL that lands on a redirecting host) therefore receives the key. The rule in `AGENTS.md` says header values must never leave through places the user did not choose.

## Expected behavior

- Vega does not follow redirects on requests that carry credentials. A `3xx` answer is reported as an error that names the status and the target, without the query string of the target, for example:
  `server redirected the request (HTTP 301) to https://swapi-graphql.netlify.app/graphql: use that URL as the endpoint`
- The same message appears wherever a request is made: the schema fetch (CLI and TUI status bar), `--run` and the Run pane.
- Nothing is sent to the redirect target.

## Open questions

- Follow same-origin `307` and `308` (they keep the method and the body) and only stop on a change of origin? It is friendlier, but more code and one more rule to explain. The simple version (never follow) is safe and enough to start with.
- Requests without any header: follow as usual, or treat all requests the same for simplicity? Treating them the same is simpler.

## Acceptance criteria

- A test with two local listeners: the first answers a redirect (301 and 307), the second records what it receives. The error names the status and the target, the exit code is non-zero, stdout stays empty, and the second listener receives no request.
- The error never contains a header value.
- The TUI shows the message in the status bar when the schema fetch is redirected.
- `docs/authentication.md` states that redirects are not followed.
