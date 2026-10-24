# Make the "no endpoint configured" error say everything needed to run vega

Status: open (not scheduled)

## Problem

Running `vega` with nothing configured prints:

```
$ vega
Error: no GraphQL endpoint configured: pass --uri, set VEGA_GRAPHQL_URI, or add `uri` to config.toml
```

This is not clear enough:

- It says nothing about `VEGA_GRAPHQL_KEY`.
- It does not say where `config.toml` lives.

## Expected behavior

The message gives clear, full-path indications of everything needed to run vega:

- the three ways to provide the endpoint (`--uri`, `VEGA_GRAPHQL_URI`, `uri` in the config file), in precedence order;
- the full resolved path of `config.toml` (from `$XDG_CONFIG_HOME/vega`, with the fallback actually in use), and whether the file exists;
- that credentials, when the server needs them, are read only from `VEGA_GRAPHQL_KEY` and `VEGA_GRAPHQL_HEADERS` (never their values).

## Acceptance criteria

- A test asserts the message contains the resolved config path and the `VEGA_GRAPHQL_KEY` name.
- The error goes to stderr and the exit code is unchanged; stdout stays empty.
- The message never includes the API key value.
- Update the README and help text if they describe configuration.

## Update after 0004

Plain `vega` no longer fails when nothing is configured: it opens the connection modal. The error described above is now printed only by the one-shot commands (`--search`, `--describe`, `--query`, `--run`), where there is no UI to fall back to. Their message is unchanged, so this issue remains open for them; the expected behavior and the acceptance criteria apply to those commands.
