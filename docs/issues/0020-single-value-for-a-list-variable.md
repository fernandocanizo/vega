# Accept a single value for a list-typed variable

Status: open (needs a decision)
Related: 0006 (find public GraphQL servers)

## Problem

Vega checks `--var` values against the variable's type, and a list type must be given a JSON list:

```
$ vega --uri https://beta.pokeapi.co/graphql/v1beta --query Query.pokemon_v2_pokemon \
    --select arg:order_by,arg:limit,name --var 'order_by={"id":"desc"}' --var limit=1
Error: variable $order_by: expected a list
```

The GraphQL specification (input coercion for lists) says a value that is not a list, and not null, is coerced to a list of one item. The server agrees: sent as `"variables": {"o": {"id": "desc"}}` for `$o: [pokemon_v2_pokemon_order_by!]`, the PokéAPI server (Hasura) returns the same data as with `[{"id": "desc"}]`. Hasura documentation uses the single-object form for `order_by`, so people will type it.

The strict check is intentional today: `crates/vega-core/src/variables.rs` has a test that expects `"x"` for a list to be invalid.

## Decision needed

- Accept a single value and wrap it in a list when the item type accepts it (what the spec and servers do), or
- keep rejecting it, but say how to fix it (`expected a list: write [ ... ]`).

## Expected behavior

Whichever is chosen, the message or the coercion is covered by a test, and the generated query shows the value that is sent.
