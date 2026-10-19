# AutoDJ Live foundation evidence

This record distinguishes execution evidence from platform and release claims.
ADR 14 remains Proposed; this document does not accept it.

## Reference Windows check

| Field | Recorded value |
| --- | --- |
| Computer | `TABLET-NCDTT0IB` |
| Operating system | Windows 11 Home `10.0.26200`, build `26200` |
| CPU | AMD Ryzen Z1 Extreme |
| Physical RAM | `16,863,055,872` bytes |
| WebView2 Runtime | `155.0.4283.62` |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`; Cargo `1.99.1` |
| Node/npm | Node `26.8.1`; npm `11.19.0` |
| Python/uv | Python `3.14.3`; uv `0.13.1` |

## Foundation process exercise

| Check | Evidence | Result |
| --- | --- | --- |
| Relocatable sidecar | `npm run live:stage` copied a complete uv-managed CPython 3.14 runtime, installed the lock-exported sidecar dependency set with hashes, then launched it with `-I` and authenticated `/live/v1/health`. | pass |
| Unsigned development bundle | `npm run live:build` produced `target/x86_64-pc-windows-msvc/release/bundle/nsis/AutoDJ Live_0.1.0_x64-setup.exe` (20.90 MiB) on this computer. | pass |
| Shell restart/reattachment | Pending an unsigned Tauri bundle exercise on this computer. Record shell, engine, and sidecar PIDs; engine/session ID; timestamps; restart-within-30-seconds result; and lease-expiry result here. | pending manual |
| Sidecar failure and normal shutdown | Pending the same manual Windows exercise; capture process IDs, timestamps, redacted logs, and result. | pending manual |
| DPAPI protected discovery | Pending installed-build verification. The development/sandbox result is not treated as installed-path evidence. | pending manual |

## Stage 1 limits and later gates

## Automated verification

- Root JavaScript: 83 files, 1,858 tests passed; root lint passed. Node heap limit was 8 GiB.
- Rust: final workspace test log completed successfully, including installed discovery tests on this host. This does not establish installed-bundle GUI behavior.
- Full Python run: 7,508 passed, 26 skipped, one stale workspace-membership assertion failed; coverage 94.21%. After adding the Tauri app to that assertion, all 45 focused workspace/sidecar/launcher tests passed. The full suite was not repeated after that test-only correction.
- Packaging scripts: Ruff checks and formatting passed; isolated staged-runtime health smoke passed.

## Remaining release evidence

Every row below is an explicit gap, not a negative test result or release
promise.

| Area | Status | Owning follow-on plan |
| --- | --- | --- |
| Audio latency measurement | gap | audio routing |
| Four-hour 48 kHz / 128-sample soak | gap | audio routing / release |
| MIDI timing, MIDI 1/2, and mappings | gap | actions and MIDI |
| ASIO and WASAPI device/routing evidence | gap | audio routing |
| Signing and update trust | gap | release |
| Installer clean-machine validation | gap | release |
| Portable bundle and offline startup | gap | release |
| Numark Mixtrack Platinum, Pioneer DDJ-1000, and Rane ONE MKII evidence | gap | decks and scratch |
| Accessibility/NVDA desktop samples | gap | performance UI / release |
