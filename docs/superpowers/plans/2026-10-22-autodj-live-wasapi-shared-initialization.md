# AutoDJ Live WASAPI Shared Initialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a DJ explicitly initialize and release a WASAPI shared buffer and see its requested and allocated frame counts without starting audio.

**Architecture:** Add a transient Media-category initialization operation to `live-audio`, transport its scalar result through the existing authenticated single-flight engine path, and expose a separate Setup action. Reuse the exact shared-mix descriptor and validated period range; retain no COM client after the operation returns.

**Tech Stack:** Rust 2024, windows 0.62.2, Tokio, MessagePack/Serde, Tauri 2, TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-22-autodj-live-wasapi-shared-initialization-design.md`

## Global Constraints

- Windows 11 24H2+ and x64 remain the first-release floor.
- The operation is explicit, single-flight, and authorized to the active supervisor.
- Set non-offloaded `AudioCategory_Media` before re-querying periods and initializing.
- Request 128 frames only when the configured client's range accepts it; otherwise request its default.
- Use the exact shared-mix descriptor and `InitializeSharedAudioStream` flags `0`.
- Never call `Start`, transfer samples, select another device, or retain the client.
- Make no ASIO, event-driven, stable-playback, callback-safety, or latency claim.
- Add no dependency and expose no native error detail to the WebView.
- Push verified commits to `feat/autodj-live-audio-routing`; do not merge or open a pull request.

## Review Focus

- A 128-frame target inside the numeric range but misaligned to the fundamental period must fall back to default.
- Client properties and the period query must occur on the same client before its single initialization.
- A zero buffer result or mismatched endpoint result must never be shown as success.
- Refresh or a second discovery generation must discard a late initialization result.
- Initialization failure, device invalidation, or a non-owner request must not leak native identifiers or errors to the WebView.

---

### Task 1: Initialize and release a validated shared buffer

**Files:**
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/lease.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: `crates/live-supervisor/src/session.rs`
- Modify: `crates/live-supervisor/tests/supervision.rs`

**Interfaces:**
- Produces: `WasapiSharedInitialization { endpoint_id: String, requested_period_frames: u32, buffer_frames: u32 }`
- Produces: `initialize_wasapi_shared(endpoint_id: &str) -> Result<WasapiSharedInitialization, &'static str>`
- Produces: `EngineCommand::InitializeWasapiShared { endpoint_id: String }`
- Produces: `EngineEvent::WasapiSharedInitialized(WasapiSharedInitialization)`
- Produces: `LiveSession::initialize_wasapi_shared(&mut self, endpoint_id: &str)`

- [ ] **Step 1: Write failing Rust tests**

Add pure tests proving aligned 128 is selected, in-range-but-misaligned 128 falls back to default, out-of-range 128 falls back to default, and zero/empty initialization evidence is rejected. Add protocol round trips, active-owner authorization, and the supervisor's native integration assertion.

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test -p live-audio -p autodj-live-protocol -p autodj-live-engine -p autodj-live-supervisor`

Expected: compilation fails because the new result, command, event, and methods do not exist.

- [ ] **Step 3: Implement the minimal native and authenticated path**

Add the pure selector/result validation. On Windows activate the requested active endpoint, validate its mix descriptor, set `AudioCategory_Media`, re-query and validate periods, choose the period, call `InitializeSharedAudioStream(0, ...)`, read `GetBufferSize`, and return validated scalar evidence. Extend the engine worker dispatch, mapping, lease authorization, protocol, and supervisor request path.

- [ ] **Step 4: Verify GREEN and commit**

Run the focused command from Step 2, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets -- -D warnings`.

Expected: all pass with no warnings.

Commit: `feat: initialize WASAPI shared buffers`

### Task 2: Expose the explicit Setup action

**Files:**
- Modify: `apps/autodj-live/src-tauri/src/lib.rs`
- Modify: `apps/autodj-live/src-tauri/tests/app_lifecycle.rs`
- Modify: `apps/autodj-live/src-tauri/tests/production_supervisor.rs`
- Modify: `apps/autodj-live/src/main.ts`
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`
- Modify: `docs/autodj-live/development.md`
- Modify: `docs/autodj-live/audio-discovery.md`

**Interfaces:**
- Consumes: Task 1's `WasapiSharedInitialization` and `LiveSession::initialize_wasapi_shared`
- Produces: Tauri command `initialize_wasapi_shared_endpoint(endpoint_id: String)`
- Produces: `AudioSetupApi.initializeShared(endpointId: string): Promise<WasapiSharedInitialization>`

- [ ] **Step 1: Write failing shell and UI tests**

Assert the shell refuses the operation before setup, passes through engine data after setup, and invokes the backend only on request. In UI tests assert separate probe/initialize actions, explicit transient-access copy, requested/allocated success text with no playback or latency claim, endpoint-ID mismatch rejection, generic failure copy, focus restoration, and stale-result rejection after refresh.

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test -p autodj-live-app` and `npm --workspace @autodj/live test`

Expected: compilation/type failures and UI failures because the new shell/API action is absent.

- [ ] **Step 3: Implement the minimal shell and UI path**

Pass the typed operation through the production backend and Tauri command. Add one button/result per active endpoint, preserve discovery-generation and focus behavior, and keep all failure text generic. Update contributor and evidence-limit documentation.

- [ ] **Step 4: Verify, gather native evidence, review, and synchronize**

Run Rust formatting, Clippy, the full Rust workspace suite, all 17+ UI tests, the app check/lint/build commands, six workspace-policy tests, architecture checks, and `git diff --check`. Run one explicit native initialization against a connected active endpoint and record only the endpoint format, requested frames, allocated frames, and the fact that the stream was never started.

Obtain one fresh whole-change review. Fix Critical/Important findings test-first and defer Minor findings. Commit as `feat: expose WASAPI shared buffer checks`, push the feature branch, and require local/remote divergence `0 0` without merging or opening a pull request.
