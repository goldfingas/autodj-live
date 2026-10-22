# AutoDJ Live WASAPI Silent Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit one-second event-driven WASAPI shared-output test that submits silence, stops cleanly, and reports bounded evidence without claiming production playback readiness.

**Architecture:** Extend the existing direct-WASAPI worker with a render-only silent exercise, carry its typed result through the authenticated engine/supervisor protocol, then expose it as an output-only Setup action. Reuse the current single-flight audio worker, compatibility handshake, discovery-generation handling, and generic error boundary.

**Tech Stack:** Rust 2024, windows-rs 0.62.2/Win32 WASAPI, Tokio, MessagePack/Serde, Tauri 2.12.3, TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-22-autodj-live-wasapi-silent-output-design.md`

## Global Constraints

- Windows 11 24H2 remains the first-release platform floor.
- The action is explicit, output-only, silent, fixed at one second, and never automatic.
- Use the exact validated shared mix format, non-offloaded `AudioCategory_Media`, and 128 frames only when the configured shared-period range accepts it; otherwise use the default.
- Initialize with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`, register a nonsignaled auto-reset event before `Start`, prefill silence, then use padding-derived available frames.
- A wait that does not signal within 250 ms is a failure; every post-start return path must call `Stop` before releasing the client.
- No new crate dependency; add only the required windows-rs feature to the existing pinned dependency.
- No music, tone, input capture, routes, exclusive mode, ASIO, MMCSS, recovery, latency, underrun, soak, or playback-readiness claim.
- Preserve authenticated owner authorization, the single-flight worker, default-false compatibility capability, generic external errors, keyboard focus, and refresh invalidation.
- Keep `feat/autodj-live-audio-routing` synchronized with its remote; do not merge or open a pull request.

## Review Focus

- Capture endpoint: reject as non-render before initialization; Task 1 tests the direction guard and Task 2 tests that capture rows have no action.
- Invalid or empty native evidence: reject rather than publishing a success; Task 1 tests every zero field and elapsed time below 1,000 ms.
- Older engine: reject locally without writing the unknown command or consuming the connection; Task 1 proves a following Health request still succeeds.
- Refresh during the one-second run: discard the late result immediately while preserving the refreshed controls; Task 2 tests both successful and failed refreshes.
- Repeated activation: disable the action while pending, issue one request, and restore keyboard focus; Task 2 tests double-click suppression and focus.

---

### Task 1: Native event-driven render and authenticated protocol

**Files:**
- Modify: `crates/live-audio/Cargo.toml`
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/compatibility.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/lease.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: `crates/live-supervisor/src/session.rs`
- Modify: `crates/live-supervisor/tests/supervision.rs`

**Interfaces:**
- Produces: `WasapiSharedOutputTest { endpoint_id: String, requested_period_frames: u32, buffer_frames: u32, event_notifications: u32, silent_frames_submitted: u64, elapsed_ms: u64 }` in `live-audio` and `autodj-live-protocol`.
- Produces: `live_audio::discovery::run_wasapi_shared_output_test(endpoint_id: &str) -> Result<WasapiSharedOutputTest, &'static str>`.
- Produces: `EngineCommand::RunWasapiSharedOutputTest { endpoint_id: String }`, `EngineEvent::WasapiSharedOutputTested(WasapiSharedOutputTest)`, and default-false `ServerHello::supports_wasapi_shared_output_test`.
- Produces: `LiveSession::run_wasapi_shared_output_test(&mut self, endpoint_id: &str) -> Result<WasapiSharedOutputTest, SupervisorError>`.

- [ ] **Step 1: Write failing native value and direction tests**

In `discovery.rs`, add tests named `shared_output_test_requires_render_direction` and `shared_output_test_requires_complete_one_second_evidence`. Assert only `Direction::Output` is accepted; valid evidence includes nonempty ID, positive period/buffer/events/frames and `elapsed_ms >= 1_000`; separately assert rejection for an empty ID and each zero field and for `elapsed_ms == 999`.

- [ ] **Step 2: Write failing protocol and compatibility tests**

Round-trip the command, event, and all exact result fields. Decode an older named `ServerHello` without the new capability and assert false. Add a supervisor socket test asserting unsupported execution returns `Rejected`, writes no frame, and leaves the connection usable for a following `Health` request. Extend lease tests so a non-owner cannot request the new operation.

- [ ] **Step 3: Verify RED**

Run: `cargo test -p live-audio -p autodj-live-protocol -p autodj-live-engine -p autodj-live-supervisor --lib --tests`

Expected: compile/test failures for the absent type, command/event, capability, method, and direction/evidence helpers.

- [ ] **Step 4: Implement the native one-second silent render**

Add windows-rs `Win32_System_Threading`. In `windows.rs`, validate active state and `IMMEndpoint::GetDataFlow() == eRender`; configure and initialize `IAudioClient3` with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`; create/register an auto-reset event; acquire `IAudioRenderClient`; prefill the allocated buffer with `AUDCLNT_BUFFERFLAGS_SILENT`; start; then until `Instant` reaches one second, wait at most 250 ms, derive writable frames as `buffer_frames - GetCurrentPadding()`, and submit nonzero space as silent. Use small owned guards for the Win32 handle and started client so all error paths close/stop. Return only the validated evidence fields.

- [ ] **Step 5: Implement the engine and supervisor path**

Add the typed mappings, owner authorization, existing single-flight worker execution, new default-false hello capability, local supervisor capability rejection, and generic `"WASAPI shared output test failed"` engine error. Do not alter the healthy connection when the capability is absent.

- [ ] **Step 6: Make the hardware success test opt-in**

Add an ignored Windows integration test using only `AUTODJ_LIVE_WASAPI_OUTPUT_ENDPOINT_ID`. Assert matching identity, positive period/buffer/events/frames, and at least 1,000 elapsed milliseconds. Do not select the first active endpoint or make hardware availability part of the default suite.

- [ ] **Step 7: Verify GREEN and commit**

Run `cargo fmt --all -- --check`, `cargo clippy -p live-audio -p autodj-live-protocol -p autodj-live-engine -p autodj-live-supervisor --all-targets -- -D warnings`, and the focused test command from Step 3. Require zero failures with only the selected-hardware test ignored.

Commit as `feat: exercise WASAPI shared silent output`, push the feature branch, and require local/remote divergence `0 0`.

### Task 2: Setup action, evidence, and production bundle

**Files:**
- Modify: `apps/autodj-live/src-tauri/src/lib.rs`
- Modify: `apps/autodj-live/src-tauri/tests/app_lifecycle.rs`
- Modify: `apps/autodj-live/src-tauri/tests/production_supervisor.rs`
- Modify: `apps/autodj-live/src/main.ts`
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`
- Modify: `docs/autodj-live/audio-discovery.md`
- Modify: `docs/autodj-live/development.md`
- Modify: `docs/autodj-live/foundation-evidence.md`

**Interfaces:**
- Consumes: Task 1's `WasapiSharedOutputTest` and `LiveSession::run_wasapi_shared_output_test`.
- Produces: Tauri command `run_wasapi_shared_output_test(endpoint_id: String)` and TypeScript `AudioSetupApi.runSharedOutputTest(endpointId: string): Promise<WasapiSharedOutputTest>`.

- [ ] **Step 1: Write failing shell lifecycle tests**

Extend the fake backend and lifecycle tests to assert setup is required, the endpoint ID passes through exactly once, the typed result passes through unchanged, and the public error remains the existing generic component failure. Update production-backend fakes for the new trait method.

- [ ] **Step 2: Write failing Setup interface tests**

Add fixtures and tests that assert: only active output rows show **Run silent output test**; pending copy is concise; success lists period, buffer, event count, silent frames, elapsed time, and the no-playback/no-latency limit; errors expose no private detail; double click makes one call; keyboard focus returns; duplicate names use distinct IDs; refresh immediately invalidates pending evidence; and a failed refresh leaves the old row/action usable without accepting the stale result.

- [ ] **Step 3: Verify RED**

Run: `cargo test -p autodj-live-app` and `npm --workspace @autodj/live test`

Expected: compile/type/test failures because the backend method, Tauri command, API member, button, and result rendering do not exist.

- [ ] **Step 4: Implement the minimal shell and UI path**

Pass the Task 1 operation through the existing setup-gated Tauri lifecycle and command registration. Add the TypeScript result type/API call and one output-only button/result per active render endpoint. Capture the current discovery generation at click time, disable while pending, ignore late generations, and restore focus only when the same controls remain current. Use ASCII `...` in new pending copy.

- [ ] **Step 5: Document exact limits and opt-in hardware command**

Document that the action submits silence for one second and stops/releases the client; it is not music, routing, latency, underrun, recovery, or stability evidence. Document the ignored hardware test command with `AUTODJ_LIVE_WASAPI_OUTPUT_ENDPOINT_ID`. If exercised locally, record the exact endpoint format and bounded returned fields without generalizing beyond that run.

- [ ] **Step 6: Run complete verification and build**

Run: `cargo fmt --all -- --check`; `git diff --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `npm --workspace @autodj/live test`; `npm --workspace @autodj/live run check`; `npm --workspace @autodj/live run lint`; `uv run pytest --no-cov tests/unit/test_live_workspace.py -v`; `uv run --frozen --no-sync python scripts/check_architecture_docs.py`; and `npm --workspace @autodj/live run build`.

Expected: every automated gate passes; the hardware success test is ignored unless the selected endpoint variable is supplied; the NSIS installer is rebuilt.

- [ ] **Step 7: Review, fix, and synchronize**

Obtain one fresh whole-change review. Fix Critical/Important findings test-first and defer Minor findings. Commit as `feat: expose WASAPI silent output test`, push the feature branch, fetch, and require divergence `0 0` with a clean tracked worktree. Preserve the branch without merging or opening a pull request.
