# Security policy

## Reporting

Open a private security advisory on this repository
(<https://github.com/0DukePan/stoolie/security/advisories/new>) or email the maintainer listed on the
GitHub profile. Please do not open a public issue for anything that could help someone misuse the
tool against another person's machine.

## What stoolie is, in security terms

A read-only reader of four local sources, plus a SQLite database it writes itself:

| Reads | Writes |
|---|---|
| `HKCU`/`HKLM` capability access store | `%LOCALAPPDATA%\stoolie\stoolie.db` |
| `Microsoft-Windows-Privacy-Auditing/Operational` (via `wevtutil`) | `receipt.html` when asked |
| `%LOCALAPPDATA%\ConnectedDevicesPlatform\*\ActivitiesCache.db` (copy) | |

It contains **no network code**: no HTTP client, no sockets, no update check, no telemetry. The
`--network` flag runs `netstat` and counts sockets owned by the process, so the claim is measured
rather than asserted.

## Threat model

- **Another person on the same machine.** The archive is plain SQLite readable by anything running as
  the user. This is deliberate — a receipt nobody can audit is not a receipt — but it means two
  people sharing an account share the archive.
- **A hostile application name or path.** Every string interpolated into the HTML receipt is escaped;
  there is a test for it (`receipt_escapes_hostile_app_names`). The receipt loads no remote content
  and runs no JavaScript.
- **Command execution.** The only child processes are `wevtutil` (fixed arguments) and `netstat`
  (fixed argument). No shell, no user-supplied command line.
- **Elevation.** The tool never requests it. A capability the user cannot read is reported as
  unavailable, not as empty.

## Out of scope

- Anything requiring a kernel driver, packet capture or TLS interception. Those are refusals, not
  missing features.
- Protecting a user from someone who already has their account.
