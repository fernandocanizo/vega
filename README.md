# Vega

Vega is a terminal GraphQL client for exploring a server's schema. It asks the server for its schema through introspection, caches it on disk, and lets you search and navigate operations and types without leaving the terminal.

It is a general GraphQL tool: it works with any server that allows introspection and, optionally, authenticates with an API key, a bearer token, basic authentication or custom HTTP headers (see [Authentication](docs/authentication.md)).

## What it does

- Search queries, mutations, subscriptions and types by part of their name.
- Open any result to read its documentation, arguments and return type, and follow a field to its type to keep exploring.
- Interactive TUI, plus command line flags (`--search`, `--describe`, `--json`) so scripts and coding agents can use the same features.
- Caches the schema per server, shows how old the cache is, and refreshes it on demand.
- Build a query by picking fields from a tree: expand with →, select with Space (optional arguments too), edit the typed variables with Tab and Enter, and watch the generated query update, press r to run it and read the response (mutations ask first).

Building, editing and running queries works; subscriptions can be built but not run.

## Features

- compose flawless queries by selecting fields directly from the documentation, Vega handles missing required parents, syntax and prettiness.
- runs entirely in the terminal, so you can explore and query a server over SSH or on a machine with no desktop or browser.
- every feature is also available as a command line flag with `--json` output, so scripts and coding agents can search the schema, describe types and generate or run queries without a UI.
- search operations and types by part of their name, then follow a field to its type to keep exploring, without opening a separate documentation panel.
- keeps the schema cached on disk and tells you how old it is, so browsing and `--search` work instantly and offline after the first fetch.
- credentials come from the environment or from the connection modal (API key, bearer token, basic authentication or any HTTP headers, see [Authentication](docs/authentication.md)): they are never written to disk, the cache or logs, and they never appear in the response pane or in `--run` output.
- starts with nothing configured: enter the endpoint and credentials in the connection modal, opened with Ctrl-K. What you type stays in memory for the session only.
- mutations ask for confirmation before they run.
- generates the query and its typed variables from the selected fields, and the same query can be printed from the command line with `--query` and `--select`.

### Planned - NOT DONE YET

- multiple server profiles, to switch between endpoints without changing flags or environment variables.
- interactive logins such as OAuth 2.0.
- skip schema refetches when the server reports that its version has not changed.
- keep the selected row centered while scrolling long lists and field trees.
- copy&paste support for queries, variables and responses.
- a position indicator (line count and percentage) when moving through long queries and responses.
- better colors, a switch to turn them off, and themes you can configure.
- export results to a file, with a download-like folder picker.

## Requirements

To build and install Vega you need:

- A recent stable [Rust](https://www.rust-lang.org/) toolchain (`cargo` and `rustc`). The easiest way to get one is [rustup](https://rustup.rs/); a package from your operating system works too. Vega is developed with Rust 1.99.
- A C linker (`gcc` or `clang`), which Rust uses to link binaries. It is usually already installed.
- [just](https://github.com/casey/just), a command runner used for the build, install and check recipes.
- [Git](https://git-scm.com/), to get the source.
- GNU [coreutils](https://www.gnu.org/software/coreutils/) `install`, used by `just install` (standard on Linux). On systems without it, copy the built binary by hand, see below.

Vega itself has no runtime dependencies: the TLS support is built in, so it does not need OpenSSL.

## Installation

```sh
git clone https://github.com/fernandocanizo/vega.git
cd vega
just install
```

`just install` builds a release binary and copies it to `~/.local/bin/vega`. Make sure `~/.local/bin` is in your `PATH`, then check:

```sh
vega --version
```

Without `just`, or without GNU `install`, build and copy the binary yourself:

```sh
cargo build --release -p vega
cp target/release/vega ~/.local/bin/    # or any directory in your PATH
```

## Configuration

Settings are read in this order: command line flag, environment variable, then config file.

| Setting         | Flag           | Environment variable      | Config file            |
| --------------- | -------------- | ------------------------- | ---------------------- |
| Endpoint URL    | `--uri`        | `VEGA_GRAPHQL_URI`        | `uri` in `config.toml` |
| API key         | none           | `VEGA_GRAPHQL_KEY`        | not supported          |
| API key header  | `--key-header` | `VEGA_GRAPHQL_KEY_HEADER` | not supported          |
| Other headers   | none           | `VEGA_GRAPHQL_HEADERS`    | not supported          |

Secrets have no flag on purpose, and `VEGA_GRAPHQL_HEADERS` holds one `Name: value` per line. See [Authentication](docs/authentication.md) for the recipes.

Files follow the [XDG base directory specification](https://specifications.freedesktop.org/basedir/latest/):

- Config: `$XDG_CONFIG_HOME/vega/config.toml` (default `~/.config/vega/config.toml`)
- Schema cache: `$XDG_CACHE_HOME/vega/` (default `~/.cache/vega/`)

Example `config.toml`:

```toml
uri = "https://example.com/graphql"
```

## Usage

```sh
export VEGA_GRAPHQL_URI="https://example.com/graphql"
export VEGA_GRAPHQL_KEY="your-api-key"

vega                              # interactive browser
vega --search user                # operations and types whose name contains "user"
vega --describe Query.userRead    # definition, docs, arguments and return type
vega --search user --json         # machine readable output
vega --query Query.userRead --select id,name   # print the generated query and its variables
vega --query Query.users --select id,arg:role --var role=ADMIN   # opt in an optional argument and set a variable
vega --query Query.userRead --select id,name --var id=42 --run   # run it and print the response
vega --refresh                    # refetch the schema instead of using the cache
```

The first run fetches the schema; later runs use the cache. The cache age is shown in the status bar of the interactive browser (red after 24 hours) and as a warning on stderr for the command line flags. Press `?` or `F1` inside the browser to see all the keys.

### Try it without a server of your own

Vega needs no configuration to start, so you can point it at a public GraphQL API. Two that work without credentials:

```sh
# Countries: small and easy to browse
vega --uri https://countries.trevorblades.com/graphql
vega --uri https://countries.trevorblades.com/graphql --search country
vega --uri https://countries.trevorblades.com/graphql --query Query.country --select name,capital --var code=AR --run

# Rick and Morty: a larger schema with pagination and filters
vega --uri https://rickandmortyapi.com/graphql
vega --uri https://rickandmortyapi.com/graphql --search character
vega --uri https://rickandmortyapi.com/graphql --query Query.character --select name,species --var id=1 --run
```

[APIs-guru/graphql-apis](https://github.com/APIs-guru/graphql-apis) keeps a longer list of public GraphQL APIs. It does not say which ones need a key or have introspection disabled, so some will not work as they are: for a key or other headers, press `Ctrl-K` in the browser or see [Authentication](docs/authentication.md). Public endpoints also change or disappear, so these examples may stop working one day.

## Development

```sh
just hooks   # enable the git hooks (checks before every commit)
just check   # format check, clippy and tests, the same command CI runs
just fix     # apply formatting and fixable lint suggestions
```

Contributor and agent conventions are in [AGENTS.md](AGENTS.md). The design is in [docs/superpowers/specs](docs/superpowers/specs/).

## About the name

Vega borrows some ideas from [Altair](https://altairgraphql.dev/) and brings them to the command line, so its name is a nod to Altair.

The two names also belong to neighboring stars. Vega is the brightest star in Lyra and Altair is the brightest star in Aquila. Together with Deneb, the brightest star in Cygnus, they form the Summer Triangle, an asterism that is easy to spot in the northern hemisphere's summer sky. Vega and Altair are the close pair of the three, at about 25 and 17 light-years from the Sun.

Their names come from Arabic: Vega from *an-nasr al-wāqi'* ("the falling eagle") and Altair from *al-nasr aṭ-ṭā'ir* ("the flying eagle"). In the East Asian legend celebrated at Qixi and Tanabata, Vega is the Weaver Girl and Altair the Cowherd, kept apart by the Milky Way and allowed to meet once a year.

Vega is also a former and future pole star. Earth's axis slowly changes direction, a wobble called axial precession that takes about 26,000 years to complete a cycle, so the star nearest the north celestial pole changes over time. Vega was the pole star around 12,000 BCE and will be again in roughly 12,000 years, around 14,000 CE, when it will pass within about 5 degrees of the pole.

## License

GNU Affero General Public License, see [LICENSE](LICENSE).
