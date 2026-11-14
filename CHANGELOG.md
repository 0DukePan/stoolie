# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-11-14

First release. Windows 10/11, x64 and arm64, no administrator rights required.

### Added

- `stoolie` — summary of sensor sessions for the last 13 days, with `--since`, `--no-color`.
- `stoolie receipt` — one self-contained HTML file: headline numbers, per-day activity strip,
  sessions with durations and context, camera verdict, privacy-setting ledger.
- `stoolie watch` — live transitions ("microphone opened — Discord.exe — 02:09:36").
- `stoolie json` — the snapshot behind the receipt, pretty or `--compact`.
- `stoolie doctor` — which data sources are readable on this machine, and why not when they are not.
- `stoolie --network` — measures the process's own outbound connections with `netstat`.
- Append-only SQLite archive at `%LOCALAPPDATA%\stoolie\stoolie.db`, imported on every run, so
  sessions survive the registry's one-slot history.
- Readers for `CapabilityAccessManager\ConsentStore` (HKCU + HKLM),
  `Microsoft-Windows-Privacy-Auditing/Operational`, and `ActivitiesCache.db`.
- Tiers for every row: recorded, reported, ledger, context, unknown.

### Known limits

- Windows only.
- The timeline context is corroboration, not attribution: records overlap by design.
- Access is not content. The tool reports that a device was open, never that anything was recorded.
- Sources that require elevation (`SRUDB.dat`, Prefetch, `powercfg /sleepstudy`) are not read; the
  first run must never show a permission prompt.
