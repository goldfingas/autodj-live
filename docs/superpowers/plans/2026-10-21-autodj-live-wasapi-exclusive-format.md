# AutoDJ Live WASAPI Exact Exclusive Format Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Report whether an active endpoint accepts its exact shared-mix descriptor in WASAPI exclusive mode, without initializing or starting a stream.

**Architecture:** Extend the existing read-only WASAPI probe and carry one boolean through the current audio, protocol, engine, supervisor, Tauri, and setup UI path. Call `IAudioClient::IsFormatSupported` with `AUDCLNT_SHAREMODE_EXCLUSIVE`, the validated `GetMixFormat` pointer, and no closest-match output; only `S_OK` means supported, while the documented unsupported-format result means unsupported and other failures keep the probe generic.

**Tech Stack:** Rust, windows-rs/Win32 WASAPI, MessagePack protocol, Tauri, TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-17-autodj-live-design.md`

## Global Constraints

- Windows only; no new dependency.
- Probe only an active endpoint chosen explicitly by the DJ.
- Do not call `IAudioClient::Initialize`, start a stream, select a device, or claim measured latency.
- The result covers only the exact shared-mix channel layout, sample rate, and sample representation; other exclusive formats and ASIO remain untested.
- Preserve generic user-facing failures and current stale-result/focus behavior.

## Review Focus

- An unsupported exact format is a successful probe with `false`, not a probe failure.
- Device invalidation and other unexpected HRESULTs fail generically instead of being reported as unsupported.
- The exact descriptor is validated before it is passed back to the driver.
- Serialization carries the new boolean through every process boundary without renaming drift.
- UI copy cannot be read as proof that all exclusive formats or ASIO work.

---

### Task 1: Probe and transport exact exclusive-format support

**Files:**
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: existing Rust fixtures that construct `WasapiCapabilities`

**Interfaces:**
- Consumes: the validated pointer returned by `IAudioClient::GetMixFormat`.
- Produces: `WasapiCapabilities::exclusive_shared_mix_supported: bool` in audio and protocol models.

- [ ] **Step 1: Write failing Rust tests**

Add assertions showing the capability model and protocol round trip retain both `true` and `false`, and isolate HRESULT classification so `S_OK` is supported, `AUDCLNT_E_UNSUPPORTED_FORMAT` is unsupported, and an unexpected failure is an error.

- [ ] **Step 2: Run targeted tests and verify they fail**

Run: `cargo test -p autodj-live-audio -p autodj-live-protocol`

Expected: FAIL because the new field and HRESULT classifier do not exist.

- [ ] **Step 3: Implement the minimal native probe and transport mapping**

After existing format validation, call `IAudioClient::IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, format_ptr, None)`. Add the boolean to `wasapi_capabilities(...)`, both structs, engine mapping, and all fixtures; map only the documented unsupported-format HRESULT to `false` and return a static generic error for other failures.

- [ ] **Step 4: Run targeted Rust checks**

Run: `cargo test -p autodj-live-audio -p autodj-live-protocol -p autodj-live-engine -p autodj-live-supervisor -p autodj-live`

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: probe exact WASAPI exclusive format`

### Task 2: Label the bounded evidence in Setup

**Files:**
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`

**Interfaces:**
- Consumes: camelCase `exclusiveSharedMixSupported: boolean` from the existing Tauri command.
- Produces: explicit supported/not-supported copy scoped to the exact displayed shared-mix format.

- [ ] **Step 1: Write failing UI tests**

Assert both boolean states render as exact-format evidence and retain “other exclusive formats and ASIO untested” language.

- [ ] **Step 2: Run the focused UI test and verify it fails**

Run: `npm --prefix apps/autodj-live test -- --run src/setup.test.ts`

Expected: FAIL because the type and label do not expose the new result.

- [ ] **Step 3: Implement the minimal label and fixture updates**

Add the field to the TypeScript interface and append one sentence that says the displayed exact shared-mix format is supported or not supported in exclusive mode; do not imply broader negotiation.

- [ ] **Step 4: Run UI checks**

Run: `npm --prefix apps/autodj-live test -- --run src/setup.test.ts && npm --prefix apps/autodj-live run build`

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: show exact WASAPI exclusive support`

### Task 3: Verify on the installed endpoint and synchronize

**Files:**
- Modify: `docs/superpowers/plans/2026-10-21-autodj-live-wasapi-exclusive-format.md`

**Interfaces:**
- Consumes: the existing `live-audio` discovery example.
- Produces: verified local and remote branch state.

- [ ] **Step 1: Run the native probe on the installed Rane endpoint**

Run the existing discovery example with probing enabled and record only the returned capability evidence; no stream may be initialized.

- [ ] **Step 2: Run repository verification**

Run formatting, Clippy with warnings denied, full Rust tests, UI tests/build, workspace policy tests, architecture checks, and whitespace checks.

- [ ] **Step 3: Mark this plan complete and commit**

Commit message: `docs: record exact WASAPI exclusive verification`

- [ ] **Step 4: Push and prove synchronization**

Push `feat/autodj-live-audio-routing`, then require `git rev-list --left-right --count HEAD...origin/feat/autodj-live-audio-routing` to print `0 0`.
