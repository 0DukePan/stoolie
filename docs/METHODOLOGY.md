# Methodology

This document exists so that a claim in the receipt can be checked instead of trusted. Every row
comes from a file Windows already wrote, read with the rights of the user who ran `stoolie`.

## Sources

| Source | Path | What it proves | Retention | Admin |
|---|---|---|---|---|
| Capability access store | `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\<capability>` | An application held a capability, with a start and a stop time | **Latest session per app**, overwritten on each new use | no |
| Machine-wide capability store | same path under `HKLM` | The same, for entries Windows keeps per machine | same | no (read) |
| Privacy auditing log | `Microsoft-Windows-Privacy-Auditing/Operational` | A consent setting changed, when, and which process changed it | months, until the log rotates | no |
| Activity timeline | `%LOCALAPPDATA%\ConnectedDevicesPlatform\<id>\ActivitiesCache.db` | Which applications had activity records, and when | days to weeks | no |
| stoolie archive | `%LOCALAPPDATA%\stoolie\stoolie.db` | Every session stoolie has imported since install | forever, until you delete it | no |

Not read, on purpose: `SRUDB.dat`, Prefetch, `powercfg /sleepstudy`, `powercfg /energy`. All of them
require elevation, and the first thing `stoolie` must never do is show a permission prompt. They are
a later, opt-in mode, not a silent requirement.

## Precision and its limits

- **Timestamps are FILETIME**, 100-nanosecond intervals since 1601 UTC. The registry stores them at
  full precision; `stoolie` keeps milliseconds, which is why the receipt says `14.1s` rather than
  `14s`.
- **`LastUsedTimeStop = 0`** means the capability is held right now. The value is not an error and
  not the 1601 epoch. `stoolie` reports it as "in use".
- **One session per app.** The registry does not keep a list. If an application opened the
  microphone five times today, the store holds the fifth session only. The archive fixes this
  going forward, not retroactively — and the receipt says which rows come from where.
- **Timeline records overlap.** At any given second, several applications can have an activity
  record. That is why context is reported as a count ("4 apps had records that second") and never
  as an attribution ("you were in Discord").
- **Absence is not evidence.** A missing row means the source did not record it, or the source is
  unavailable, or `stoolie` was not running. It does not mean nothing happened.
- **Access is not recording.** A microphone can be open and silent; a call can hold the device for
  forty minutes. The product reports device access and stops there.

## What the tiers mean

| Tier | Meaning |
|---|---|
| Recorded | The archive observed the transition itself, while running |
| Reported | A start and a stop are present in an OS artefact; both are printed |
| Ledger | A consent value changed; the change and caller are shown, not the usage |
| Context | Another app had activity records at that time; corroboration only |
| Unknown | The source is missing; printed with the reason, never guessed |

## How the archive behaves

- Append-only: `INSERT OR IGNORE` on `(capability, app, start)`. Once a session is stored, a later
  run cannot rewrite its stop time, even if the OS disagrees.
- Imported on every run, from whatever the registry still holds.
- Plain SQLite, no encryption, no upload, no background process. If you copy the file, you copy the
  receipts.
- Deleted only when you delete the file.

## Reproducing a receipt

```
stoolie doctor          # what is readable on this machine
stoolie json            # the exact data behind the receipt
stoolie receipt         # the rendered artefact
```

`stoolie json` is the source of truth: if the receipt and the JSON disagree, the JSON is the bug
report.

## Privacy of the tool itself

- No network code. `stoolie --network` measures the process's own sockets and prints zero.
- No telemetry, no crash reporting, no update ping in the default path.
- The archive is readable by anything running as you. That is deliberate: a receipt nobody can audit
  is not a receipt.
