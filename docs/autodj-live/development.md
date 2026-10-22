# AutoDJ Live development

AutoDJ Live is an optional Windows desktop workspace. It does not change the
existing `autodj serve` workflow, and contributors working only on AutoDJ's
browser/server application do not need the Live toolchain.

## Prerequisites

Install these manually; no Live command installs a compiler, WebView runtime,
or driver:

- Windows 11 with the Microsoft C++ Build Tools (MSVC x64/x86 and a Windows SDK);
- Microsoft Edge WebView2 Runtime;
- Rust `1.99.1`, MSVC host `x86_64-pc-windows-msvc`, plus `rustfmt` and `clippy`;
- Node.js 26 (the CI image pins `26.12.0`); and
- Python 3.14 and `uv`.

Install repository dependencies from the root, then run the non-installing
prerequisite check:

```powershell
uv sync --frozen --all-extras
npm ci --ignore-scripts
npm run live:check
```

`live:check` verifies versions and the MSVC Rust host only. It does not start
the application or alter system configuration.

## Focused checks

Read-only Windows endpoint and x64 ASIO registration inventory:

```powershell
cargo run -p live-audio --example discover
```

This diagnostic opens no streams and loads no ASIO driver. All capabilities
remain unprobed. It includes disconnected/disabled endpoints and preserves
partial results when metadata fails; exit status 1 means one or more discovery
issues were reported, not necessarily that the entire inventory failed.
The command-line form remains useful as a developer diagnostic.

The desktop Setup section exposes the same read-only inventory through the
authenticated Live engine. It performs no automatic scan: activate **Refresh
audio devices** to replace the current list. Endpoint availability and ASIO
registration are inventory only; no item is selectable and no capabilities,
channel layout, or latency are implied. The IPC wait is bounded; a stalled scan
may continue outside the async runtime and blocks further scans until it exits.
Partial metadata issues remain visible without discarding healthy entries.

For an active endpoint, **Probe WASAPI** reports the current shared-engine mix
format and default/minimum engine periods. It also checks whether that exact
descriptor is accepted in exclusive mode and tests the official 44.1, 48, and
96 kHz rates while preserving the descriptor's channel layout and sample
representation. For the exact shared format it also reports the default,
fundamental, minimum, and maximum shared-engine wake-up periods returned by
`IAudioClient3`, including whether 128 frames is selectable. It activates the
Windows audio client only for those queries and never initializes or starts a
stream. The result does not establish support for other channel layouts,
sample representations, ASIO, physical channel names, an initialized buffer
size, stable playback, or measured latency. Refreshing the inventory clears
probe results. When the shell reattaches to an older compatible engine, Setup
marks absent evidence unavailable instead of treating it as driver rejection.

`cargo test -p live-audio` checks control-thread device and channel-route
validation, including separate master/cue paths. This library does not open
hardware, render audio, publish routes to a callback, or establish ASIO/WASAPI
support or latency guarantees.

Use the smallest relevant command while iterating, then run the foundation
gate before handoff:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --workspace @autodj/live test
npm --workspace @autodj/live run lint
uv run pytest --no-cov tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py -v
uv run --frozen --no-sync python scripts/check_architecture_docs.py
```

`npm run live:stage` creates `target/live-sidecar/python`: a copied uv-managed
CPython 3.14 standard-library/runtime plus only the lock-derived sidecar
dependencies and AutoDJ sidecar source. It immediately launches that relocated
runtime with Python isolated mode and verifies the authenticated health route.
The normal project virtual environment is never bundled.

## Development build

```powershell
npm run live:build
```

The build is an unsigned development NSIS artifact. It stages the Python
sidecar before Tauri bundles resources. It is not a release claim: signing,
offline WebView packaging, portable distribution, updates, driver installation,
and clean-machine installer validation remain release-stage work.

## Fork synchronization

The working fork is `goldfingas/autodj-live` (`origin`); the source repository is
`blindndangerous/AutoDJ` (`upstream`). Fetch before starting a work session, push
verified commits to the matching topic branch, and verify that local HEAD and
its remote-tracking branch agree. Uncommitted work is not backed up by a push.

Keep fork `master` aligned with upstream using fast-forward updates. Integrate
upstream changes into Live topic branches explicitly and test them; do not
force-push or automatically merge divergent histories. Continue development on
topic branches without opening a pull request until the operator requests one.
