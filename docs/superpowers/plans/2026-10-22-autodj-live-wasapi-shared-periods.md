# AutoDJ Live WASAPI Shared-Period Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Report the shared-mode WASAPI engine-period range for an active endpoint's exact mix format and whether the 128-frame target is selectable, without initializing a stream.

**Architecture:** Extend the existing explicit read-only probe by casting its activated client to `IAudioClient3` and querying `GetSharedModeEnginePeriod` with the already validated mix-format descriptor. Validate and transport the four frame counts, preserving older-engine compatibility with an optional protocol field, then render narrowly scoped evidence in Setup.

**Tech Stack:** Rust, windows-rs/Win32 WASAPI, MessagePack protocol, Tauri, TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-22-autodj-live-wasapi-shared-periods-design.md`

## Global Constraints

- Pass the exact validated `GetMixFormat` descriptor to the period query.
- Do not call `Initialize`, `InitializeSharedAudioStream`, `Start`, or any buffer service.
- Do not select an endpoint or change Windows settings.
- Period evidence is not actual buffer size, stable playback, or measured latency.
- Add no dependency and make no ASIO capability claim.
- Preserve compatibility with an attached older protocol-1 engine.

## Review Focus

- Zero or incoherent period values must fail generically before transport.
- A period is selectable only when it is within the inclusive min/max range and is an integral multiple of the fundamental period.
- The exact mix-format pointer must remain alive and unchanged throughout the query.
- Missing evidence from an older engine must not be presented as a queried rejection of 128 frames.
- UI copy must not imply stream initialization, actual buffer size, playback stability, or latency measurement.

---

### Task 1: Query and transport validated shared periods

**Files:**
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/compatibility.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: Rust fixtures constructing protocol `WasapiCapabilities`

**Interfaces:**
- Produces: `SharedModeEnginePeriods { default_frames: u32, fundamental_frames: u32, minimum_frames: u32, maximum_frames: u32 }` in audio and protocol models.
- Produces: `SharedModeEnginePeriods::supports_period_frames(frames: u32) -> bool` in `live-audio`.
- Produces: `WasapiCapabilities.shared_mode_engine_periods: Option<SharedModeEnginePeriods>` in the protocol; new engines send `Some`.

- [x] **Step 1: Write failing Rust tests**

Add tests for a valid `480/4/48/480` range; 128 supported; values below/above the range and nonmultiples rejected; zero, reversed, out-of-range default, and nonmultiple returned values invalid. Add protocol populated round-trip and missing-field compatibility assertions.

- [x] **Step 2: Run targeted tests and verify missing-interface failures**

Run: `cargo test -p live-audio -p autodj-live-protocol`

Expected: FAIL because the period model and field do not exist.

- [x] **Step 3: Implement the query and transport**

Validate the four counts in a pure constructor. Cast the existing client to `IAudioClient3`, call `GetSharedModeEnginePeriod(format_ptr, ...)`, and pass the validated model through the audio result, protocol `Some`, engine mapping, and fixtures. Keep the format allocation alive until the query returns.

- [x] **Step 4: Run focused verification**

Run `cargo fmt --check`, workspace clippy with warnings denied, and `cargo test --workspace`.

- [x] **Step 5: Commit and push the native checkpoint**

Commit as `feat: probe WASAPI shared engine periods`, push `feat/autodj-live-audio-routing`, and require divergence `0 0`.

### Task 2: Present bounded period evidence and verify hardware

**Files:**
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`
- Modify: `docs/autodj-live/development.md`
- Modify: `docs/autodj-live/audio-discovery.md`
- Modify: this plan

**Interfaces:**
- Consumes: `sharedModeEnginePeriods: { defaultFrames: number; fundamentalFrames: number; minimumFrames: number; maximumFrames: number } | null`.
- Produces: distinct supported, unsupported, and unavailable 128-frame text with explicit evidence limits.

- [x] **Step 1: Write failing UI tests**

Assert a present aligned range reports the four frame values and 128-frame availability; a range with 160-frame minimum reports the target unavailable; `null` reports probing unavailable. All paths retain stream/buffer/playback/latency limits.

- [x] **Step 2: Run the focused UI test and verify it fails**

Run: `npm --prefix apps/autodj-live test -- --run src/setup.test.ts`

Expected: FAIL because Setup does not render shared-period evidence.

- [x] **Step 3: Implement Setup copy and documentation**

Add the nullable TypeScript field and pure label calculation. Update contributor docs to distinguish engine wake-up constraints from initialized buffer size or latency.

- [x] **Step 4: Run native and full verification**

Probe the connected active Rane endpoint without opening a stream. Run Rust formatting/clippy/tests, UI tests/check/lint/build, six workspace policy tests, architecture traceability, and `git diff --check`.

- [ ] **Step 5: Review, commit, push, and prove synchronization**

Obtain one fresh whole-change review; fix Critical/Important findings test-first and defer Minor findings. Commit as `feat: show WASAPI shared engine periods`, push the feature branch, and require local/remote divergence `0 0`. Preserve the branch without merging or opening a pull request.

**Native evidence:** On 2026-10-22 the connected active `Mic 1/2 (Rane
PERFORMER)` endpoint returned a 2-channel, 48 kHz, 32-bit shared format and
480-frame default/fundamental/minimum/maximum shared engine periods. The
128-frame target was not selectable for that exact format. No stream was
initialized or started.
