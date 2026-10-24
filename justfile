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

# List issues by status: open (default), doing, done or all.
issues status="open":
    #!/usr/bin/env bash
    set -euo pipefail
    for f in docs/issues/[0-9]*.md; do
        state=$(sed -n 's/^Status: *//p' "$f" | head -1)
        word=$(printf '%s' "$state" | cut -d' ' -f1 | tr '[:upper:]' '[:lower:]')
        title=$(sed -n 's/^# *//p' "$f" | head -1)
        if [ "{{status}}" = all ] || [ "$word" = "{{status}}" ]; then
            printf '%s  %-34s  %s\n' "$(basename "$f" | cut -d- -f1)" "$state" "$title"
        fi
    done
