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
It is a developer diagnostic, not yet exposed through the desktop setup UI.

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
