default: check

# Format check, lints (warnings are errors) and tests.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# Apply formatting and machine-fixable lints.
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# Build in release mode and copy the binary to ~/.local/bin (XDG user executables).
install:
    cargo build --release -p vega
    install -Dm755 target/release/vega "$HOME/.local/bin/vega"

# Point git at the repository hooks.
hooks:
    git config core.hooksPath githooks
