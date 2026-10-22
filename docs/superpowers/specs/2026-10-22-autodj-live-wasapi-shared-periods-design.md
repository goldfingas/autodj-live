# AutoDJ Live WASAPI Shared-Period Design

## Intent

Prepare AutoDJ Live for low-period shared-mode WASAPI streaming by reporting
the engine wake-up periods Windows accepts for an explicitly probed endpoint's
exact shared mix format. This gives the DJ useful evidence before any stream is
initialized and establishes whether the 128-frame club target is selectable.

## Goals

- Query the default, fundamental, minimum, and maximum shared-mode engine
  periods in frames through `IAudioClient3::GetSharedModeEnginePeriod`.
- Validate the returned range before it crosses the engine boundary.
- Show whether 128 frames is within the range and aligned to the fundamental
  period.
- Preserve reconnection compatibility with older protocol-1 engines by
  representing absent period evidence separately from a queried result.

## Non-goals and anti-goals

- Do not initialize or start a stream, request a period, select an endpoint, or
  change a Windows audio setting.
- Do not describe engine wake-up periods as buffer size, measured latency, or
  proof of stable playback.
- Do not add an audio-host dependency or begin ASIO SDK integration here.
- Do not use the instantaneous value from
  `GetCurrentSharedModeEnginePeriod`; another client can invalidate it as soon
  as it is read, so it is not durable setup evidence.

## Constraints

- Windows 11 24H2 remains the first-release floor; `IAudioClient3` has existed
  since Windows 10.
- The exact validated descriptor returned by `GetMixFormat` is passed to the
  query unchanged.
- All four frame counts must be nonzero. Minimum, default, and maximum must be
  ordered and each must be an integral multiple of the fundamental period.
- Failures remain generic at the IPC/UI boundary and no COM or endpoint detail
  is exposed.

## Approaches

1. **Query `IAudioClient3` for the exact shared format (chosen).** This adds no
   dependency, has no stream side effect, and produces the exact constraints
   needed by a later `InitializeSharedAudioStream` checkpoint.
2. **Report the current shared-engine period.** This is cheaper data but is
   instantaneous and can change immediately, so it cannot guide a saved setup.
3. **Initialize a silent test stream.** This would exercise more of the path,
   but it also introduces endpoint ownership, callbacks, cleanup, device loss,
   and audible-session behavior before those lifecycles are designed.

## Design

`live-audio` adds `SharedModeEnginePeriods` with `default_frames`,
`fundamental_frames`, `minimum_frames`, and `maximum_frames`, plus a pure
`supports_period_frames` check. The existing explicit WASAPI probe casts its
already activated client to `IAudioClient3`, queries the validated mix-format
pointer, validates the result, and returns it with the existing capabilities.

The protocol carries `Option<SharedModeEnginePeriods>` with a serde default.
New engines always send `Some`; a missing field from an older compatible
engine decodes as `None`. Setup renders the four values and the 128-frame
result when present, or says shared-period probing is unavailable when absent.
Copy states that the figures are engine wake-up periods and that stream
initialization, actual buffer size, playback stability, and end-to-end latency
remain untested.

## Evidence

Unit tests cover malformed period ranges and 128-frame alignment. Protocol
tests cover populated round trips and missing-field compatibility. UI tests
cover supported, unsupported, and unavailable 128-frame evidence. A native
probe records one connected Rane result without opening a stream, followed by
the existing Rust, UI, policy, architecture, and production-installer gates.

## Primary sources

- [IAudioClient3](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient3)
- [GetSharedModeEnginePeriod](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient3-getsharedmodeengineperiod)
- [InitializeSharedAudioStream](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient3-initializesharedaudiostream)
