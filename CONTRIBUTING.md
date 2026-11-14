# Contributing

Thanks for looking. A few rules keep this project honest.

## The one rule

**Never claim more than the data proves.** If a change makes a receipt say something the source does
not support — an attribution where only corroboration exists, a recording where only access exists —
it will be rejected, however good the screenshot looks. There is a test named
`receipt_escapes_hostile_app_names`; the naming is a clue about how seriously unsupported strings are
treated here.

## Before opening a pull request

```powershell
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all
```

All three must be clean. Windows-only crate: the CI runs on `windows-latest` with the MSVC toolchain,
while `rust-toolchain.toml` pins the GNU host for local builds.

## What is welcome

- New readers for artefacts that are readable **without elevation** (see `docs/METHODOLOGY.md`).
- Fixes to parsing edge cases, with a fixture that reproduces them. Fixtures must be captured from a
  real machine with the machine name, user name and SID removed.
- Receipt and terminal copy improvements, especially empty states.
- Documentation that makes a claim checkable.

## What is out of scope

Packet capture, TLS interception, blocking/firewall rules, kernel drivers, eBPF, cloud sync, accounts,
a background service, an auto-updater. Each is a refusal, not a backlog item.

## Adding a data source

1. Read `docs/METHODOLOGY.md` and add a row: path, what it proves, retention, admin requirement.
2. Implement it as a reader returning domain types, with a tier.
3. Add a `doctor` line so the user can see whether the source works on their machine.
4. Add a test with a fixture, including the failure path (missing source, odd value, locked file).

## Commit style

Imperative subject, then a paragraph explaining *why* the change exists. "Fix parsing" is not a
commit message; "Decode the # separators so Win32 app paths survive into the receipt" is.
