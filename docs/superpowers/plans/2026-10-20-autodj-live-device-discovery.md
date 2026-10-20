# AutoDJ Live Device Discovery Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans, with test-first implementation and a final independent review.

**Goal:** Read-only enumeration of Windows endpoints and x64 ASIO registrations.
**Architecture:** Extend live-audio with a discovery module using the existing
windows 0.62.2 dependency. Run COM enumeration on a dedicated thread, return
owned data and per-source/per-device errors, and expose a diagnostic example.
No audio client activation, driver initialization, streams, or settings writes.
**Spec:** `docs/autodj-live/audio-discovery.md` and the approved Live design.

## Global Constraints

- Windows 11 24H2+, x64; engine-owned future integration.
- Keep existing browser/server playback unchanged.
- Names are labels, not identity; capabilities remain explicitly unprobed.
- Enumerate only 64-bit ASIO registration; never load a driver for inventory.
- No automatic device selection, rerouting, fallback, or SDK installation.
- Push verified changes without a PR or master merge.

## Review Focus

Invalid CLSID text must not escape as valid registrations; duplicate
display names must retain separate IDs; disabled/unplugged endpoints must not
appear active; partial failures must not erase healthy entries; COM and registry
resources must be released on errors.

### Task 1: Read-only discovery

**Files:** `crates/live-audio/src/discovery.rs`,
`crates/live-audio/src/discovery/windows.rs`,
`crates/live-audio/examples/discover.rs`, library exports, manifest/lockfile,
and discovery documentation.

**Interfaces:** `discover() -> Discovery`, holding `endpoints`, `asio_drivers`,
and `issues`. Endpoint fields: `id`, `name`, `direction`, `state`.
ASIO fields: `registration`, `clsid`; presence means registered, not usable.
No fabricated `DeviceCapabilities` is produced.

- [ ] Write tests for endpoint-state mapping and strict API-returned UTF-16 decoding:
  empty, missing terminator, embedded NUL, invalid surrogate, valid CLSID text.
- [ ] Observe tests fail before implementing the conversion functions.
- [ ] Enumerate MMDevice capture/render endpoints in all states using read-only
  property stores; use IDs when names cannot be read and preserve the error.
- [ ] Enumerate HKLM SOFTWARE\ASIO with KEY_WOW64_64KEY and read CLSID values.
  Missing root is an empty inventory; access errors are not empty success.
- [ ] Add a diagnostic example showing directions, states, registrations and
  issues, always stating that capabilities are unprobed.
- [ ] Run focused tests, fmt, clippy, full Rust workspace tests, Python workspace
  policy tests, architecture checks, and a real read-only discovery run.
- [ ] Obtain independent review, resolve significant findings, commit, push,
  and verify local/remote equality.

UI/IPC exposure, capability probing, loading ASIO, audio rendering, MIDI, and
hardware performance qualification remain follow-on work.

## Verification checkpoint (2026-10-20)

Implemented on `feat/autodj-live-audio-routing`. Both new test batches were
observed failing for missing functions before their implementation.
`cargo test --workspace` passed 60 tests. After the review wording/test-name
clarification, all 11 live-audio tests passed again. Formatting, live-audio
Clippy with warnings denied, six Python workspace-policy tests, architecture
structure checks, and whitespace checks passed. Full Python/JavaScript suites
were not rerun for this native-library-only change.

The actual diagnostic returned 61 endpoints and 9 x64 ASIO registrations,
with 12 unavailable names on stale endpoints and exit status 1 for partial
metadata. See `docs/autodj-live/audio-discovery.md` for limitations.

## Review disposition

Ruling: retain `RegGetValueW` and accept its documented insertion of a missing
REG_SZ terminator. Validate the returned buffer and GUID rather than claiming
raw storage validation. The original wording overstated the boundary; a
syntactically valid CLSID repaired this way is safe to list as unprobed.
Cost: this diagnostic cannot identify that specific raw registry defect.
No driver is loaded and no usability claim follows from this acceptance.
All other independent review checks had no findings; actual OS failure
injection, hardware usability, and driver compatibility remain unverified.
