# AutoDJ Live WASAPI Shared Initialization Design

Date: 2026-10-22
Status: Approved by the maintainer's standing instruction to continue without
extended design questions

## Intent

Move the Windows audio path from read-only capability inspection to one real,
user-requested shared-stream allocation. AutoDJ Live will initialize an active
endpoint's shared-mix format, read the allocated buffer size, and immediately
release the client without starting audio.

This checkpoint proves only that Windows accepted one transient shared-mode
initialization at the reported period. It does not prove rendering, capture,
event-callback behavior, stable playback, underrun handling, device recovery,
or end-to-end latency.

## Goals

- Apply `AudioCategory_Media` before period negotiation and initialization.
- Re-query the engine-period range after applying the client properties.
- Request 128 frames when it is valid for that range; otherwise request the
  reported default period.
- Initialize with the endpoint's exact shared-mix descriptor and stream flags
  `0`, read `IAudioClient::GetBufferSize`, then release every COM resource.
- Carry the endpoint ID, requested period, and allocated buffer frames through
  the authenticated engine, supervisor, Tauri, and Setup UI path.
- Require a separate DJ action and describe the transient hardware access
  before it occurs.

## Non-goals and anti-goals

- Do not call `Start`, `Stop`, `GetService`, `GetCurrentPadding`, or transfer
  audio samples.
- Do not claim event-driven operation, measured latency, glitch-free playback,
  route publication, or callback safety.
- Do not select a Windows default device, change system settings, retain the
  initialized client, or retry on another endpoint.
- Do not load ASIO drivers or generalize this evidence to ASIO or exclusive
  mode.
- Do not add a dependency or expose native error details to the WebView.

## Approaches considered

### Route editor first

Expose the existing route validator in Setup. This would make configuration
visible but still leave the selected routes unable to open hardware, so it
does not advance the riskiest audio boundary.

### Shared initialization check (selected)

Add one transient `IAudioClient3::InitializeSharedAudioStream` operation and
report its actual allocated frames. This is the smallest real hardware step
that reuses the existing discovery, probe, authorization, and single-flight
paths while remaining silent and recoverable.

### Event-driven render loop

Create an event, render silence continuously, and add start/stop lifecycle.
That immediately introduces a real-time thread, buffer servicing, shutdown,
underrun evidence, and device-loss behavior. Those concerns need their own
checkpoint and tests.

## Design

`live-audio` adds `WasapiSharedInitialization` and a pure period selector. The
selector chooses 128 only when the existing inclusive-range and fundamental-
alignment predicate accepts it; otherwise it chooses the validated default.
The result constructor rejects an empty endpoint ID or zero frame counts.

The Windows implementation activates a fresh `IAudioClient`, reads and
validates the complete shared-mix descriptor, casts to `IAudioClient3`, sets a
non-offloaded Media-category `AudioClientProperties`, then queries periods on
that same configured client. It initializes once with flags `0`, obtains the
buffer capacity, and returns owned scalar evidence. Rust ownership releases
the initialized COM client and task-allocated mix format on every exit path.

The authenticated protocol gains one command and one result event. The engine
uses the existing audio-worker single-flight gate and active-supervisor
authorization. The supervisor and Tauri shell pass the typed result through
without native diagnostics.

After endpoint discovery, every active endpoint shows a separate **Check
shared buffer** action. Nearby copy states that it briefly initializes and
releases a silent shared buffer. Success text names the requested and allocated
frames and states that the stream was never started. Refreshing discovery
invalidates late initialization results just as it invalidates probe results.

## Validation and evidence limits

Pure Rust tests cover target/default selection and malformed results. Protocol,
lease, supervisor, shell, and UI tests cover the typed path, authorization,
user initiation, stale-result rejection, generic failures, and bounded copy.
One native manual check may establish initialization on the named connected
endpoint and format. It cannot establish stable operation, latency, event-mode
support, or behavior on another device.

## Sources

- Microsoft `IAudioClient3::InitializeSharedAudioStream` documents the period
  constraints and that buffer size is calculated from the requested period.
- Microsoft `IAudioClient::GetBufferSize` requires successful initialization
  and returns the allocated capacity in frames.
- Microsoft audio processing-mode guidance requires `SetClientProperties`
  after activation and before initialization; Media represents music playback.
