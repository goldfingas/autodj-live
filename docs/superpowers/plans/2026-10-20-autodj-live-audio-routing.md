# AutoDJ Live Audio Route Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish tested device-capability and channel-route validation before hardware playback is enabled.

**Architecture:** Add a dependency-free `live-audio` Rust workspace library owned by the future engine audio host. Compile caller-provided routes into a validated immutable configuration on the control thread. This first slice neither opens devices nor changes playback.

**Tech Stack:** Existing Rust workspace and standard library only.

**Spec:** `docs/superpowers/specs/2026-10-17-autodj-live-design.md`, Audio engine sections.

**Implementation checkpoint (2026-10-20):** Tasks 1 and 2 are implemented on
`feat/autodj-live-audio-routing`; the checklist below preserves the original
test-first instructions. Both test batches first failed for missing APIs.
Independent read-only review found no critical, important, or minor issues.
Verification passed: `cargo test --workspace` (55 tests), `cargo fmt --check`,
`cargo clippy -p live-audio --all-targets -- -D warnings`, six Python
workspace-policy tests, architecture-document structure, and `git diff --check`.
Hardware/backend support, callback safety, atomic publication, device loss,
input-graph feedback, latency, and release readiness remain excluded; this
configuration-only slice provides no evidence for those later requirements.

## Global Constraints

- Windows 11 24H2 remains the first-release platform floor.
- Official sample rates are 44.1, 48, and 96 kHz.
- The engine, not Python or the WebView, owns audio.
- One device is the real-time clock master.
- Master, booth, cue, record, stream, and per-deck paths remain distinct.
- No vendor drivers, new dependencies, or hardware activation in this slice.
- Validation is control-thread work; do not claim callback safety or latency from these tests.
- Use a topic branch based on the reviewed foundation; do not merge into master or open a PR.
- Push tested commits and verify local/remote equality. Fetch before work; do not force-push.

## Review Focus

- Unsupported rates: reject rather than silently resampling (Task 1).
- Invalid channel indices, including zero-channel devices: reject without indexing panics (Task 2).
- Duplicate source-to-output edges: reject accidental gain doubling (Task 2).
- Separate buses sharing physical outputs: permit intentional summing without erasing bus identity (Task 2).
- Invalid replacement: return an error without modifying the caller's active configuration (Task 2).

## Scope and files

Create `crates/live-audio/Cargo.toml`, `src/lib.rs`, `src/routing.rs`,
and `tests/routing.rs`. Update workspace membership in `Cargo.toml`,
`Cargo.lock`, and the exact-member assertion in `tests/unit/test_live_workspace.py`.
Confirm that test filename with repository discovery before editing.
Update `docs/autodj-live/development.md` with the focused test command and limits.

This intentionally covers only the first part of the audio stage. Subsequent
plans must cover ASIO SDK/license review, WASAPI discovery and negotiation,
real-time rendering, atomic route publication/reclamation, hardware inputs,
feedback detection across the full graph, secondary-device drift bridges,
device loss, recording/stream handoffs, IPC/UI setup, and measured hardware
evidence. None is implied by a validated channel map.

### Task 1: Device capability validation

**Files:** workspace manifest/lockfile; new library manifest, `src/lib.rs`,
`src/routing.rs`, `tests/routing.rs`; existing workspace-policy test.

**Interfaces:**

- `DeviceCapabilities { sample_rates: Vec<u32>, output_channels: u16 }`
- `validate_device(capabilities: &DeviceCapabilities, sample_rate: u32) -> Result<(), RouteError>`
- `RouteError::{UnsupportedSampleRate, NoOutputChannels, ChannelOutOfRange, DuplicateRoute}`

- [ ] Add the crate to the workspace using existing edition/toolchain conventions; expose `pub mod routing`.
- [ ] Write failing tests: 44100/48000/96000 are accepted only when present in capabilities; 22050 is rejected even when advertised; an empty supported-rate list is rejected; zero output channels returns `NoOutputChannels` for an otherwise supported rate.
- [ ] Run `cargo test -p live-audio --test routing`; confirm failure is missing validation, not toolchain configuration.
- [ ] Implement the named types and validation. Check official-rate membership and advertised support before channel count.
- [ ] Run the focused Rust tests and the existing workspace-policy test; require zero failures.
- [ ] Commit as `feat: validate Live audio device capabilities`, push the topic branch, verify ahead/behind `0 0`.

### Task 2: Immutable output route compilation

**Files:** `src/routing.rs`, `tests/routing.rs`, development documentation.

**Interfaces:**

- `Bus::{Master, Booth, Cue, Record, Stream, DeckAPre, DeckAPost, DeckBPre, DeckBPost}`
- `OutputRoute { bus: Bus, source_channel: u8, device_channel: u16 }`, all channel indices zero-based.
- `ValidatedRoutes` with private fields and read-only `sample_rate(&self) -> u32`, `routes(&self) -> &[OutputRoute]`.
- `compile_routes(capabilities: &DeviceCapabilities, sample_rate: u32, routes: &[OutputRoute]) -> Result<ValidatedRoutes, RouteError>`

For this slice all logical buses are stereo; source channels must be 0 or 1.
An empty route list represents intentional silence. Multiple different buses
may target one hardware channel; identical edges are forbidden. The compiled
value owns its routes, preserves their order, and cannot mutate its input.
There is no graph feedback because destinations are hardware outputs only;
do not generalize this into feedback protection for future input routing.

- [ ] Write failing tests for stereo master on channels 0/1 and cue on 2/3 with a four-channel device; reject the same map on a two-channel device.
- [ ] Add tests rejecting source channel 2, device channel equal to channel count, and identical edges; accept distinct buses sharing an output and empty routes.
- [ ] Add a replacement test: retain a valid active value, attempt invalid compilation, assert its sample rate and routes remain unchanged.
- [ ] Run the focused tests and verify the new tests fail for missing compilation.
- [ ] Implement the exact interfaces, reusing Task 1 device validation. Allocate only during compilation; make no runtime publication claim.
- [ ] Run `cargo fmt --check`, `cargo clippy -p live-audio --all-targets -- -D warnings`, `cargo test -p live-audio`, and the workspace-policy test, one at a time.
- [ ] Document the focused command and the explicit no-playback/no-hardware limits.
- [ ] Run architecture-document checks and `git diff --check`; commit as `feat: validate Live master and cue output routes`, push, verify `0 0`.

## Handoff

Report exactly which tests ran. Hardware playback, ASIO/WASAPI, route UI,
callback-time behavior, and measured latency remain unimplemented. Next is
device discovery/backend selection with primary-source API and license review,
not an automatic opening of the user's audio hardware.
