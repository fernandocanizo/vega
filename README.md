# Vega

Vega is a terminal GraphQL client for exploring a server's schema. It asks the server for its schema through introspection, caches it on disk, and lets you search and navigate operations and types without leaving the terminal.

It is a general GraphQL tool: it works with any server that allows introspection and, optionally, authenticates with an API key sent in the `X-API-Key` header.

## What it does

- Search queries, mutations, subscriptions and types by part of their name.
- Open any result to read its documentation, arguments and return type, and follow a field to its type to keep exploring.
- Interactive TUI, plus command line flags (`--search`, `--describe`, `--json`) so scripts and coding agents can use the same features.
- Caches the schema per server, shows how old the cache is, and refreshes it on demand.

Composing and running queries is planned but not implemented yet.

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

| Setting      | Flag    | Environment variable | Config file          |
| ------------ | ------- | -------------------- | -------------------- |
| Endpoint URL | `--uri` | `VEGA_GRAPHQL_URI`   | `uri` in `config.toml` |
| API key      | none    | `VEGA_GRAPHQL_KEY`   | not supported        |

The API key is only read from the environment: it is never written to disk, the cache or logs, and there is deliberately no flag for it because command line arguments are visible to other users.

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
vega --refresh                    # refetch the schema instead of using the cache
```

The first run fetches the schema; later runs use the cache. The cache age is shown in the status bar of the interactive browser (red after 24 hours) and as a warning on stderr for the command line flags. Press `?` or `F1` inside the browser to see all the keys.

## Development

```sh
just hooks   # enable the git hooks (checks before every commit)
just check   # format check, clippy and tests, the same command CI runs
just fix     # apply formatting and fixable lint suggestions
```

Contributor and agent conventions are in [AGENTS.md](AGENTS.md). The design is in [docs/superpowers/specs](docs/superpowers/specs/).

## License

GNU Affero General Public License, see [LICENSE](LICENSE).
