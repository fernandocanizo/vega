# Plaintext fallback for profiles on hosts without a keyring

Status: open (needs brainstorming)
Related: 0003 (saved profiles)

## Problem

Saved profiles keep their secret in the OS keyring. A host without one (a minimal server, a container, an SSH session with no Secret Service) cannot save a profile: Vega refuses and says why. Those users have to keep using environment variables.

## Candidates

- A plaintext file with mode `0600` and a visible warning each time it is used.
- An encrypted file unlocked with a passphrase.
- Command references such as `pass show ...` instead of the secret.

## Questions to settle

- Which candidate fits the rules in `AGENTS.md` (header values never written to a file Vega writes, today)? The rule would need to change for the first two.
- Is it opt-in per profile or a global setting?
- How does the TUI show that a profile is stored outside the keyring?

## Deliverable

A short design note with a recommendation and the tradeoffs, then implementation issues split from it. No code in this issue.
