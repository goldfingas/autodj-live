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
| Shell restart/reattachment | Release executable process exercise on 2026-10-20 below: shell restart retained engine identity and increased generation; no-restart engine exit took 30.03 seconds. | pass for local release executable |
| Sidecar failure and normal shutdown | Release executable process exercise below: engine survived sidecar termination; normal window close reaped both children and removed discovery. | pass for local release executable |
| DPAPI protected discovery | Release executable created an `Installed` protected record and successfully reattached using it. Installer deployment on a clean machine remains untested. | pass for local release executable |

### Local release executable exercise, 2026-10-20

These checks used `target/x86_64-pc-windows-msvc/release/autodj-live.exe`
and its staged resources on the reference computer, without running the
installer. The prior WebView startup access error did not recur. The shell
created a native window titled `AutoDJ Live`. This is process/lifecycle
evidence, not visual, keyboard, WebView-reload, or screen-reader certification.

All times below are EDT (UTC-04:00). Only processes started for this exercise
were terminated. Runtime credentials were neither printed nor saved here.

- Startup: shell PID `27580`, engine PID `33692`, sidecar PID `34956`;
  engine ID `29fe673c-9336-4bbe-b691-558df76a9cf6`, generation `1`.
- Crash/restart: terminated shell at `01:22:12.817`; by `01:22:14.795`,
  replacement shell `20720` retained engine `33692` and its engine ID,
  advanced to generation `2`, and started sidecar `35196`. The old sidecar
  had exited.
- Normal close at `01:22:21.914`: `CloseMainWindow` was accepted; shell,
  engine and sidecar exited, and the runtime record was removed.
- Lease expiry: shell `22644`, engine `5088`, sidecar `35580`, engine ID
  `89ee0283-7655-4fa7-88e9-f0bb9fe664fa`. Shell termination at
  `01:22:34.282` was followed by engine exit at `01:23:04.309`
  (30.03 seconds). Sidecar exited; the stale discovery record remained for
  validation on the next launch, as designed.
- Sidecar failure at `01:23:37.465`: shell `30472`, engine `5340`,
  sidecar `6420`. Terminating the sidecar left the engine running.
  Normal window close then exited the shell and engine and removed discovery.

One initial validation-script attempt read discovery during its replacement
window and exited early; the corrected script waited for a published record
before accessing process IDs. No product change was needed for this harness
race. WebView reload and first-launch-after-expiry UI status remain unobserved.

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
