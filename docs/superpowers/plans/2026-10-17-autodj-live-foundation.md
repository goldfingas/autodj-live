# AutoDJ Live Foundation Implementation Plan

**Checkpoint (2026-10-20):** This is the original implementation checklist;
its unchecked boxes are not a current completion ledger. Foundation code and
review fixes are committed on `feat/autodj-live-foundation`. See
`docs/autodj-live/foundation-evidence.md` for verification and outstanding
manual checks. The operator has authorized ongoing commit/push synchronization
to `goldfingas/autodj-live`, with no pull request yet. Further stages must remain
on topic branches; the original merge prerequisite below does not authorize
merging into `master`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the approved AutoDJ Live desktop, process, protocol, and repository foundation without changing existing AutoDJ playback.

**Architecture:** Add a Rust workspace containing a versioned IPC contract, a separately supervised Live engine process, and a testable process supervisor used by a Tauri 2 shell. Add an authenticated loopback Python sidecar that exposes only Live-owned health/catalog bootstrap surfaces. The first vertical slice launches all three components, survives a WebView or shell restart for a bounded lease, reports status accessibly, and leaves the current `autodj` CLI and web application unchanged.

**Tech Stack:** Rust 1.99.1 (MSVC), Tauri 2.12, Tokio, Serde/MessagePack, Python 3.14, FastAPI, vanilla TypeScript/ES modules with Vite 8, Vitest, pytest, GitHub Actions Windows runners.

**Spec:** `docs/superpowers/specs/2026-10-17-autodj-live-design.md`

## Global Constraints

- AutoDJ Live is AGPLv3 and remains in this repository.
- The first-release platform floor is Windows 11 24H2; this plan targets `x86_64-pc-windows-msvc` only.
- Existing AutoDJ browser/server workflows remain installable and runnable without Rust, Tauri, MIDI, or ASIO components.
- The engine is a separate process; audio/controller ownership may never move into the WebView or Python event loop.
- Control channels bind only to `127.0.0.1` on ephemeral ports and require a fresh 256-bit per-launch bearer token before returning state.
- Tokens never appear in command-line arguments, logs, crash reports, startup descriptors, or UI text.
- The engine survives a shell crash for a 30-second reconnection lease, starts no new automation while orphaned, and exits safely when the lease expires.
- Python sends complete timestamped transition plans in later stages; the IPC contract must not assume streamed time-critical actions.
- No network-audio, MIDI, ASIO, DSP, deck, or stem implementation belongs in this foundation plan.
- Existing ADR ownership, architecture checks, 90% Python coverage, Node 26, and accessibility contracts remain in force.
- Do not publish an issue, commit, push, or install developer prerequisites without operator authorization.

## Review Focus

- A forged or stale token must receive no health or process information; pinned by Task 3 engine protocol tests and Task 5 sidecar tests.
- A second launch while an engine lease is active must reattach to the matching session rather than spawn duplicate real-time owners; pinned by Task 6 supervisor integration tests.
- Engine or sidecar stdout noise must not corrupt the one-line startup descriptor; pinned by Tasks 4 and 5 subprocess tests.
- A WebView reload or Tauri shell restart must preserve engine identity while stale sessions expire safely; pinned by Task 6 lease tests and Task 7 UI reconnect test.
- Missing Rust/Tauri prerequisites must produce an actionable development error without changing existing AutoDJ startup; pinned by Task 2 prerequisite tests and Task 8 CI checks.

---

## Planned file structure

- `Cargo.toml` — Rust workspace membership and shared release/lint settings.
- `rust-toolchain.toml` — pinned Rust 1.99.1 MSVC toolchain.
- `crates/live-protocol/` — transport-neutral versioned messages and framed MessagePack codec.
- `crates/live-engine/` — separate engine executable, authenticated control listener, lease, and health state; no audio yet.
- `crates/live-supervisor/` — child discovery, launch, reattachment, health, and shutdown used by Tauri.
- `apps/autodj-live/` — Tauri package, vanilla TypeScript UI, and app-specific tests/configuration.
- `src/autodj/live_sidecar.py` — minimal authenticated FastAPI sidecar application factory.
- `src/autodj/live_sidecar_cli.py` — private subprocess entrypoint and one-line startup descriptor.
- `tests/unit/test_live_sidecar.py` — sidecar authentication and import-boundary tests.
- `tests/integration/test_live_sidecar_process.py` — descriptor and subprocess lifecycle tests.
- `docs/adr/0014-separate-autodj-live-realtime-ownership.md` — Proposed architecture decision and contracts.
- `docs/autodj-live/foundation-evidence.md` — reference-machine fields, commands, evidence limits, and Stage 1 sign-off.
- `.github/workflows/ci.yml` — Windows-only Rust/Tauri quality and build lane.

### Task 1: Record the Live ownership decision

**Files:**
- Create: `docs/adr/0014-separate-autodj-live-realtime-ownership.md`
- Modify: `docs/adr/README.md`
- Modify: `docs/superpowers/specs/2026-10-17-autodj-live-design.md`
- Test: `tests/unit/test_architecture_docs.py`

**Interfaces:**
- Consumes: Accepted ADR 3/4/5 boundaries and Proposed ADR 8/10/11/12 evidence limits.
- Produces: Stable constraint IDs `ADR-0014-01` through `ADR-0014-06` for process ownership, loopback authentication, orphan lease, compatibility, persistence, and manual platform evidence.

- [ ] **Step 1: Write the Proposed ADR**

Define `ADR-0014-01` engine ownership, `-02` authenticated loopback, `-03` 30-second orphan lease/no-new-automation rule, `-04` existing AutoDJ dependency isolation, `-05` protocol-version rejection, and `-06` manual Windows process/restart evidence. Link the approved spec and mark all implementation evidence as gaps until its owning task lands.

- [ ] **Step 2: Add ADR 14 to the index and link the spec back to it**

List it as Proposed, governed by maintainer approval. Replace the spec's prose-only ADR requirement with a local link to ADR 14.

- [ ] **Step 3: Run structural validation**

Run: `uv run --frozen --no-sync python scripts/check_architecture_docs.py`

Expected: `Structure and traceability OK`; ADR 14 rows are reported as gaps/manual evidence, not falsely passing automation.

- [ ] **Step 4: Commit**

```bash
git add docs/adr/0014-separate-autodj-live-realtime-ownership.md docs/adr/README.md docs/superpowers/specs/2026-10-17-autodj-live-design.md
git commit -m "docs: propose AutoDJ Live process ownership"
```

### Task 2: Establish reproducible Rust and Tauri workspace tooling

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.cargo/config.toml`
- Create: `apps/autodj-live/package.json`
- Create: `apps/autodj-live/vite.config.ts`
- Create: `apps/autodj-live/tsconfig.json`
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `.gitignore`
- Test: `tests/unit/test_live_workspace.py`

**Interfaces:**
- Consumes: Node `>=26`, Python `>=3.14,<3.15`, Windows `x86_64-pc-windows-msvc`.
- Produces: root Cargo workspace; npm workspace `@autodj/live`; scripts `live:dev`, `live:build`, `live:test`, and `live:check`.

- [ ] **Step 1: Write the failing workspace policy tests**

Create `test_live_workspace_pins_toolchain_and_keeps_python_package_optional()` and `test_live_npm_scripts_are_namespaced()` in `tests/unit/test_live_workspace.py`. Assert Rust `1.99.1`, resolver `2`, release `panic = "abort"`, the MSVC target, an npm workspace under `apps/autodj-live`, and no Rust/Tauri dependency added to `pyproject.toml`.

- [ ] **Step 2: Run the focused tests to verify failure**

Run: `uv run pytest --no-cov tests/unit/test_live_workspace.py -v`

Expected: FAIL because the workspace files and scripts do not exist.

- [ ] **Step 3: Add the workspace files and package metadata**

Use a root workspace with members `crates/live-protocol`, `crates/live-engine`, `crates/live-supervisor`, and `apps/autodj-live/src-tauri`. Pin `@tauri-apps/cli` `2.12.3` and Vite `8.3.4` in the Live npm workspace. Keep the existing frontend commands unchanged.

- [ ] **Step 4: Add an actionable prerequisite check**

Add `live:check` that verifies `rustc 1.99.1`, Cargo, the MSVC host, Node 26, and Tauri CLI without installing anything. Its missing-Rust error must point to Tauri's Windows prerequisites and leave `autodj --help` functional.

- [ ] **Step 5: Run policy and existing launcher tests**

Run: `uv run pytest --no-cov tests/unit/test_live_workspace.py tests/unit/test_launcher.py -v`

Expected: PASS.

- [ ] **Step 6: Install approved prerequisites only after operator confirmation, then verify metadata**

Run: `npm ci --ignore-scripts && npm run live:check`

Expected: all pinned tool versions printed; no application process launched.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .cargo/config.toml apps/autodj-live/package.json apps/autodj-live/vite.config.ts apps/autodj-live/tsconfig.json package.json package-lock.json .gitignore tests/unit/test_live_workspace.py
git commit -m "build: add AutoDJ Live workspace tooling"
```

### Task 3: Define the versioned engine protocol

**Files:**
- Create: `crates/live-protocol/Cargo.toml`
- Create: `crates/live-protocol/src/lib.rs`
- Create: `crates/live-protocol/src/codec.rs`
- Create: `crates/live-protocol/tests/round_trip.rs`
- Create: `crates/live-protocol/tests/compatibility.rs`

**Interfaces:**
- Consumes: a 32-byte bearer token supplied by the supervisor; monotonic timestamps in nanoseconds.
- Produces: `ProtocolVersion { major: u16, minor: u16 }`, `ClientHello`, `ServerHello`, `HealthSnapshot`, `EngineCommand`, `EngineEvent`, `FrameCodec::read`, and `FrameCodec::write`.

- [ ] **Step 1: Write failing round-trip and compatibility tests**

Test that every foundation message round-trips through a four-byte big-endian length-prefixed MessagePack frame; frames over 1 MiB are rejected; unknown fields survive minor-version decoding where marked; wrong major versions return `ProtocolError::IncompatibleMajor`; and debug output redacts `BearerToken`.

- [ ] **Step 2: Run the crate tests to verify failure**

Run: `cargo test -p autodj-live-protocol`

Expected: FAIL because the crate and types do not exist.

- [ ] **Step 3: Implement the public protocol types**

Use `ProtocolVersion::CURRENT = { major: 1, minor: 0 }`. Define `ClientHello { version, session_id, token }`, `ServerHello { version, engine_id, pid, lease_deadline_ns }`, `HealthSnapshot { engine_id, state, orphaned, supervisor_generation, started_ns, lease_deadline_ns }`, `EngineCommand::{Health, Attach { supervisor_generation }, Detach, Shutdown}`, and `EngineEvent::{Attached, Health, Stopping, Error}`.

- [ ] **Step 4: Implement the bounded frame codec**

Expose async `FrameCodec::read<R: AsyncRead + Unpin>() -> Result<EngineCommand, ProtocolError>` and `FrameCodec::write<W: AsyncWrite + Unpin, T: Serialize>()`. Reject zero-length and over-limit frames before allocating their payload.

- [ ] **Step 5: Run formatting, lint, and tests**

Run: `cargo fmt --check && cargo clippy -p autodj-live-protocol --all-targets -- -D warnings && cargo test -p autodj-live-protocol`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/live-protocol
git commit -m "feat: define AutoDJ Live engine protocol"
```

### Task 4: Build the separately supervised engine process

**Files:**
- Create: `crates/live-engine/Cargo.toml`
- Create: `crates/live-engine/src/main.rs`
- Create: `crates/live-engine/src/server.rs`
- Create: `crates/live-engine/src/lease.rs`
- Create: `crates/live-engine/src/startup.rs`
- Create: `crates/live-engine/tests/process_lifecycle.rs`
- Create: `crates/live-engine/tests/authentication.rs`

**Interfaces:**
- Consumes: environment variables `AUTODJ_LIVE_TOKEN` (64 lowercase hex characters), `AUTODJ_LIVE_SESSION_ID` (UUID), and optional `AUTODJ_LIVE_RECONNECT_MS` (test-only override).
- Produces: executable `autodj-live-engine`; first stdout line `{"protocol":{"major":1,"minor":0},"port":N,"pid":N,"engine_id":"UUID"}`; authenticated protocol server on `127.0.0.1:N`.

- [ ] **Step 1: Write failing authentication and descriptor tests**

Assert the server binds only IPv4 loopback, emits exactly one parseable descriptor line before accepting clients, sends no state for missing/wrong tokens, rejects stale session IDs and incompatible majors, and never includes the token in stdout/stderr.

- [ ] **Step 2: Write failing lease lifecycle tests**

With a 250 ms test lease, attach generation 1, detach, verify health reports `orphaned = true`, reattach generation 2 before expiry and keep the same `engine_id`, then detach again and verify the process exits after expiry. Assert no command other than health/attach/shutdown is accepted while orphaned.

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p autodj-live-engine --tests`

Expected: FAIL because the binary and lifecycle do not exist.

- [ ] **Step 4: Implement startup validation and descriptor output**

Parse and immediately zeroize the token source after constructing `BearerToken`. Bind `127.0.0.1:0`, write one descriptor JSON line to stdout, flush it, and route all later diagnostics to stderr through redacting fields.

- [ ] **Step 5: Implement authenticated connections and the lease state machine**

Allow one active supervisor generation. A higher generation replaces the prior connection; the same or lower stale generation is rejected. Disconnect starts the 30-second production lease. Lease expiry emits `Stopping` to any attached observer and exits cleanly.

- [ ] **Step 6: Run crate gates**

Run: `cargo fmt --check && cargo clippy -p autodj-live-engine --all-targets -- -D warnings && cargo test -p autodj-live-engine --tests`

Expected: PASS with no network listener outside loopback.

- [ ] **Step 7: Commit**

```bash
git add crates/live-engine
git commit -m "feat: add supervised AutoDJ Live engine"
```

### Task 5: Add the authenticated Python Live sidecar

**Files:**
- Create: `src/autodj/live_sidecar.py`
- Create: `src/autodj/live_sidecar_cli.py`
- Create: `tests/unit/test_live_sidecar.py`
- Create: `tests/integration/test_live_sidecar_process.py`
- Modify: `src/autodj/cli.py`

**Interfaces:**
- Consumes: `AUTODJ_LIVE_TOKEN`, `AUTODJ_LIVE_SESSION_ID`, `--host 127.0.0.1`, and `--port 0` from the private `_live-sidecar` CLI.
- Produces: `create_live_sidecar_app(*, token_digest: bytes, session_id: UUID) -> FastAPI`; authenticated `GET /live/v1/health`; first stdout descriptor line with protocol, port, PID, and sidecar ID.

- [ ] **Step 1: Write failing application tests**

Test `/live/v1/health` returns 401 with the same generic body for missing, malformed, wrong, or stale-session credentials; returns version/session/sidecar health for the correct bearer token; adds `Cache-Control: no-store`; binds no station routes; and importing `autodj.live_sidecar` does not import Torch, FAISS, sounddevice, or the music model.

- [ ] **Step 2: Write failing subprocess tests**

Start `_live-sidecar` with a token in the environment. Assert one descriptor line, loopback/ephemeral binding, token redaction, stderr-only diagnostics, graceful SIGINT/CTRL_BREAK shutdown, and a nonzero exit with an actionable message when the token is absent.

- [ ] **Step 3: Run tests to verify failure**

Run: `uv run pytest --no-cov tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py -v`

Expected: FAIL because the sidecar modules and command do not exist.

- [ ] **Step 4: Implement the minimal app factory and constant-time authentication**

Hash the supplied token once at startup. Compare bearer tokens with `hmac.compare_digest`, validate `X-AutoDJ-Live-Session`, and keep exception/log output free of header values. Do not construct `StationRegistry`, Player, MixBus, or model objects.

- [ ] **Step 5: Implement the private CLI process**

Register `_live-sidecar` as a hidden Click command. Use a pre-bound loopback socket so port `0` can be reported before Uvicorn serves. Emit the descriptor once, then give Uvicorn the socket. Do not add a new project script or change `autodj serve`.

- [ ] **Step 6: Run focused Python gates**

Run: `uv run pytest --no-cov tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py tests/unit/test_launcher.py -v`

Expected: PASS.

- [ ] **Step 7: Run lint and type checks for changed Python**

Run: `uv run ruff check src/autodj/live_sidecar.py src/autodj/live_sidecar_cli.py tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py && uv run mypy src/autodj/live_sidecar.py src/autodj/live_sidecar_cli.py`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/autodj/live_sidecar.py src/autodj/live_sidecar_cli.py src/autodj/cli.py tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py
git commit -m "feat: add authenticated AutoDJ Live sidecar"
```

### Task 6: Implement process launch, reattachment, and shutdown supervision

**Files:**
- Create: `crates/live-supervisor/Cargo.toml`
- Create: `crates/live-supervisor/src/lib.rs`
- Create: `crates/live-supervisor/src/discovery.rs`
- Create: `crates/live-supervisor/src/process.rs`
- Create: `crates/live-supervisor/src/session.rs`
- Create: `crates/live-supervisor/tests/supervision.rs`
- Create: `crates/live-supervisor/tests/reattach.rs`

**Interfaces:**
- Consumes: paths to `autodj-live-engine` and the bundled Python executable/module; startup descriptors from Tasks 4 and 5.
- Produces: `LiveSupervisor::launch(config) -> Result<LiveSession, SupervisorError>`, `LiveSession::health()`, `LiveSession::detach()`, `LiveSession::shutdown()`, and `LiveSupervisor::reattach(runtime_record)`.

- [ ] **Step 1: Write failing launch and cleanup tests**

Use fixture child binaries to assert one token/session is generated, secrets travel only in child environments, descriptors have a 5-second timeout and 8 KiB limit, malformed/noisy descriptors terminate the child, stderr is bounded, and partial startup shuts down the component that did start.

- [ ] **Step 2: Write failing duplicate-owner and reattachment tests**

Launch a real test engine, persist a private runtime record containing engine ID, port, session ID, and encrypted/protected token, drop the supervisor, then reattach generation 2 within the lease. Assert the same PID/engine ID, one engine process, stale generation rejection, and safe expiry after the lease.

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p autodj-live-supervisor --tests`

Expected: FAIL because the supervisor does not exist.

- [ ] **Step 4: Implement bounded child startup**

Use explicit executable paths and argument arrays, never a shell. Limit descriptor read length/time, drain bounded stderr, and return typed errors that do not include child environment values.

- [ ] **Step 5: Implement private runtime discovery**

Store the runtime record under the per-user app runtime directory. Protect the token with Windows DPAPI for installed builds; use an owner-only portable runtime subdirectory and refuse reattachment when privacy cannot be established. Atomic replacement preserves the previous valid record on failure.

- [ ] **Step 6: Implement generation-based attach/detach/shutdown**

Normal app exit sends `Shutdown`; shell crash/disconnect sends no shutdown and permits the 30-second lease. A fresh shell increments `supervisor_generation` and reattaches. Remove the runtime record only after confirmed engine exit or proven stale PID/engine mismatch.

- [ ] **Step 7: Run Rust gates**

Run: `cargo fmt --check && cargo clippy -p autodj-live-supervisor --all-targets -- -D warnings && cargo test -p autodj-live-supervisor --tests`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/live-supervisor
git commit -m "feat: supervise AutoDJ Live processes"
```

### Task 7: Add the accessible Tauri shell and foundation status surface

**Files:**
- Create: `apps/autodj-live/index.html`
- Create: `apps/autodj-live/src/main.ts`
- Create: `apps/autodj-live/src/status.ts`
- Create: `apps/autodj-live/src/styles.css`
- Create: `apps/autodj-live/src/status.test.ts`
- Create: `apps/autodj-live/src-tauri/Cargo.toml`
- Create: `apps/autodj-live/src-tauri/build.rs`
- Create: `apps/autodj-live/src-tauri/src/lib.rs`
- Create: `apps/autodj-live/src-tauri/src/main.rs`
- Create: `apps/autodj-live/src-tauri/tauri.conf.json`
- Create: `apps/autodj-live/src-tauri/capabilities/main.json`
- Create: `apps/autodj-live/src-tauri/tests/app_lifecycle.rs`

**Interfaces:**
- Consumes: `LiveSupervisor` from Task 6.
- Produces: Tauri commands `get_foundation_status() -> FoundationStatus`, `retry_component(component)`, and `shutdown_live()`; UI states `starting`, `ready`, `degraded`, `reattaching`, and `stopped`.

- [ ] **Step 1: Write failing UI state tests**

Test that engine and sidecar state have visible text, the same change produces one polite announcement, polling identical state is silent, degraded state names the failed component and action, Retry retains focus, ticking uptime is outside live regions, and no token/port/PID appears in accessible or visible text.

- [ ] **Step 2: Write failing Tauri lifecycle tests**

Using a fake supervisor, assert setup launches once, a second window does not spawn children, WebView reload returns the same engine ID, shell restart reattaches during the lease, Retry targets only the failed component, and normal exit requests orderly shutdown.

- [ ] **Step 3: Run tests to verify failure**

Run: `npm --workspace @autodj/live test && cargo test -p autodj-live-app --tests`

Expected: FAIL because the app and UI do not exist.

- [ ] **Step 4: Implement the least-privilege Tauri shell**

Register only the three foundation commands. Grant the main window only those capabilities; deny shell execution, arbitrary filesystem access, opener, updater, and network permissions. Start supervision in Tauri setup and expose sanitized status DTOs.

- [ ] **Step 5: Implement the accessible status surface**

Use semantic headings, definition lists, native buttons, visible focus, reduced-motion and forced-colors styles, one `role="status"` region, and the existing one-announcement principle. The page is a foundation diagnostic, not the final club layout.

- [ ] **Step 6: Run UI, Rust, and accessibility tests**

Run: `npm --workspace @autodj/live test && npm --workspace @autodj/live run lint && cargo test -p autodj-live-app --tests && cargo clippy -p autodj-live-app --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 7: Run a manual restart check on Windows**

Start the development app, record the engine ID, reload the WebView, terminate only the shell process, restart within 30 seconds, and confirm the same engine ID reaches `ready`. Repeat without restart and confirm the engine exits after the lease. Record exact processes and timestamps in `docs/autodj-live/foundation-evidence.md`.

- [ ] **Step 8: Commit**

```bash
git add apps/autodj-live
git commit -m "feat: add AutoDJ Live desktop foundation"
```

### Task 8: Add Windows CI, development guidance, and Stage 1 evidence

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `CONTRIBUTING.md`
- Create: `docs/autodj-live/development.md`
- Create: `docs/autodj-live/foundation-evidence.md`
- Modify: `docs/adr/0014-separate-autodj-live-realtime-ownership.md`

**Interfaces:**
- Consumes: all Task 1-7 checks and artifacts.
- Produces: reproducible Windows CI lane, prerequisite/setup guidance, unsigned development build, and explicit Stage 1 evidence/gaps.

- [ ] **Step 1: Add a failing workflow-policy test**

Extend `tests/unit/test_live_workspace.py` with `test_ci_has_windows_live_gate()` asserting a Windows job runs Rust fmt, clippy, workspace tests, Live npm tests/lint, Python sidecar tests, and an unsigned `npm run live:build` without changing the existing Python OS matrix.

- [ ] **Step 2: Run the policy test to verify failure**

Run: `uv run pytest --no-cov tests/unit/test_live_workspace.py::test_ci_has_windows_live_gate -v`

Expected: FAIL because the job does not exist.

- [ ] **Step 3: Add the Windows Live CI job**

Use official Rust stable setup pinned to 1.99.1, Node 26.12.0, Python 3.14, and `uv`. Cache Cargo and npm inputs. Run architecture validation before builds. Upload the unsigned development bundle and test reports; do not add release signing secrets to pull-request CI.

- [ ] **Step 4: Document prerequisites and non-installing checks**

Document MSVC C++ build tools, WebView2, Rust 1.99.1 MSVC, Node 26, Python 3.14, `npm run live:check`, and focused commands. State that Windows setup is manual and that normal AutoDJ contributors do not need the Live toolchain.

- [ ] **Step 5: Record Stage 1 evidence and remaining gates**

The evidence file names the exact tested computer, Windows build, CPU/RAM, WebView2, Rust/Node/Python versions, process IDs, restart/lease outcomes, and limitations. Leave audio latency, 128-sample soak, MIDI, ASIO, signing, installer, portable, and hardware-controller rows explicitly `gap` for their later plans.

- [ ] **Step 6: Update ADR 14 evidence mappings**

Map automated constraints to exact Rust/Python/UI selectors and keep Windows restart as manual evidence. Do not mark the ADR Accepted.

- [ ] **Step 7: Run the foundation verification set**

Run:

```bash
uv run --frozen --no-sync python scripts/check_architecture_docs.py
uv run pytest --no-cov tests/unit/test_live_workspace.py tests/unit/test_live_sidecar.py tests/integration/test_live_sidecar_process.py tests/unit/test_launcher.py -v
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --workspace @autodj/live test
npm --workspace @autodj/live run lint
npm run lint
npm test
```

Expected: all commands PASS; architecture output still lists manual/gap evidence honestly.

- [ ] **Step 8: Build the unsigned Windows development bundle**

Run: `npm run live:build`

Expected: Tauri build succeeds on Windows and produces an unsigned development installer artifact. Signing, offline WebView packaging, the portable bundle, and update installation remain later-plan gaps.

- [ ] **Step 9: Run the full existing Python gate before handoff**

Run: `uv run python scripts/ci_pytest.py`

Expected: PASS with coverage at or above 90%.

- [ ] **Step 10: Commit**

```bash
git add .github/workflows/ci.yml CONTRIBUTING.md docs/autodj-live docs/adr/0014-separate-autodj-live-realtime-ownership.md tests/unit/test_live_workspace.py
git commit -m "ci: verify AutoDJ Live foundation"
```

## Follow-on plans

Do not fold these into this branch. Each begins only after the foundation plan
is approved, merged, and its interfaces are stable:

1. `autodj-live-audio-routing` — ASIO/WASAPI, real-time graph, channel routing,
   input strips, bridges, recording taps, device loss, and audio evidence.
2. `autodj-live-actions-midi` — action registry, Windows MIDI Services/fallback,
   MIDI 1/2, Clock/MTC, mappings, feedback, auto-detection, and editors.
3. `autodj-live-decks-scratch` — decode compatibility, decks, beat grids,
   waveforms, ownership plans, touch/motorized scratch, and controller evidence.
4. `autodj-live-performance-ui` — club view, library/queue, keyboard editor,
   routing setup, screen-reader workflows, and visual audits.
5. `autodj-live-advanced-performance` — full effect baseline, racks, sampler,
   bundled offline stems, official DJ exports, and stream handoff.
6. `autodj-live-release` — recovery drills, full hardware/soak matrix, signed
   installer and portable builds, idle-only updater, security/license review,
   and release evidence.
