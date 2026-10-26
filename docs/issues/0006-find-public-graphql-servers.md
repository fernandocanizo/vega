# Find public GraphQL servers to test Vega against

Status: done (no list kept: the APIs-guru repository serves as the list; the bug hunt is recorded below)

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

## Starting points

- [APIs-guru/graphql-apis](https://github.com/APIs-guru/graphql-apis) lists about 70 public GraphQL APIs. It does not say which need credentials or allow introspection, so each one still has to be checked with vega.
- Already checked with vega (2026-10-24), no credentials needed, and used as the examples in the README: `https://countries.trevorblades.com/graphql` and `https://rickandmortyapi.com/graphql`.
- Stale URLs happen: `https://swapi-graphql.netlify.app/.netlify/functions/index` answers a POST with a 301 to `/graphql`. Vega now reports it (`server redirected the request (HTTP 301) to https://swapi-graphql.netlify.app/graphql`, see 0015), which is also the working endpoint.


## Result (2026-10-26)

No document of servers was written: [APIs-guru/graphql-apis](https://github.com/APIs-guru/graphql-apis) is the list for people who want something to try Vega on. What this issue was for, finding what the fixture cannot show, was done as a bug hunt.

26 endpoints were tried (`--refresh --search ""`, an isolated cache), taken from that list plus a few well-known ones. 11 loaded: Countries, Rick and Morty, SWAPI, AniList, PokéAPI (Hasura, about 5,500 operations and types), GitLab (941 operations, 4,589 types), TCGdex, EAN-Search, EHRI, Canopy and a Barcelona transit API. The other 15 failed for reasons outside Vega: moved or dead URLs (redirects to other hosts, 404, 405, DNS failure, a TLS handshake that `curl` also fails), or a key now required (GitHub, Buildkite, Bitquery, Digitransit).

On the 11 that loaded, about 1,000 `--describe` calls and 280 generated queries over random types and operations showed no crash, no wrong output and no slow call. The TUI stayed responsive on the GitLab schema, and `Ctrl-A` stopped at its field limit with a clear message. `--run` worked against real servers, including union branches, optional arguments, custom ID scalars and JSON object variables.

Bugs and gaps found, each filed on its own:

- 0018: no `User-Agent`, so api.github.com answers 403 to every request.
- 0019: a closed pipe (`vega --search "" | head`) panics.
- 0020: a single value for a list-typed variable is rejected, but the spec and servers accept it.
- 0021: HTML error pages are printed as the error message.
- 0022: deprecated fields are not marked where fields are listed.

Not covered: servers with introspection disabled, and servers that need a real key (no key was available).
