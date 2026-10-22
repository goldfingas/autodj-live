// @vitest-environment happy-dom

import { beforeEach, describe, expect, test, vi } from "vitest";

import { mountAudioSetup, type AudioDiscovery } from "./setup";

const DISCOVERY: AudioDiscovery = {
  endpoints: [
    { id: "output-1", name: "Speakers", direction: "output", state: "active" },
    { id: "output-2", name: "Speakers", direction: "output", state: "disabled" },
    { id: "input-1", name: "Mic 1/2", direction: "input", state: "active" },
  ],
  asioDrivers: [
    { registration: "Rane PERFORMER ASIO", clsid: "{6e9aac96-aeda-4f9a-b222-13ea83facee8}" },
  ],
  issues: ["One stale endpoint name was unavailable"],
};

const CAPABILITIES = {
  endpointId: "output-1",
  channels: 8,
  sampleRateHz: 48_000,
  bitsPerSample: 32,
  defaultPeriod100ns: 100_000,
  minimumPeriod100ns: 30_000,
  exclusiveSharedMixSupported: true,
  exclusiveSharedFormatSampleRatesHz: [44_100, 48_000],
  sharedModeEnginePeriods: {
    defaultFrames: 480,
    fundamentalFrames: 4,
    minimumFrames: 48,
    maximumFrames: 480,
  },
};

const INITIALIZED = {
  endpointId: "output-1",
  requestedPeriodFrames: 128,
  bufferFrames: 480,
};

describe("audio setup discovery", () => {
  beforeEach(() => document.body.replaceChildren());

  test("waits for the DJ to request discovery", () => {
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    const probe = vi.fn().mockResolvedValue(CAPABILITIES);
    const initializeShared = vi.fn().mockResolvedValue(INITIALIZED);
    mountAudioSetup(document.body, { discover, probe, initializeShared });

    expect(document.querySelector("h2")?.textContent).toBe("Audio setup");
    expect(document.querySelector("button")?.textContent).toBe("Refresh audio devices");
    expect(discover).not.toHaveBeenCalled();
    expect(probe).not.toHaveBeenCalled();
    expect(initializeShared).not.toHaveBeenCalled();
  });

  test("initializes a shared buffer only on request and states the evidence limits", async () => {
    const initializeShared = vi.fn().mockResolvedValue(INITIALIZED);
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe: vi.fn(),
      initializeShared,
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-initialize-wasapi-shared]")).toHaveLength(2));
    expect(document.body.textContent).toContain(
      "Checking a shared buffer briefly initializes and releases it. Audio is never started.",
    );
    const initialize = document.querySelector("[data-initialize-wasapi-shared]") as HTMLButtonElement;
    initialize.focus();
    initialize.click();

    expect(initializeShared).toHaveBeenCalledWith("output-1");
    expect(initialize.disabled).toBe(true);
    await vi.waitFor(() => expect(initialize.disabled).toBe(false));
    expect(document.activeElement).toBe(initialize);
    expect(document.body.textContent).toContain(
      "Shared buffer initialized and released: requested 128 frames Â· allocated 480 frames. The stream was never started; playback stability and latency remain untested.",
    );
    expect(document.querySelector("[data-audio-announcement]")?.textContent)
      .toBe("WASAPI shared buffer check ready.");
  });

  test("rejects mismatched shared-buffer evidence with generic copy", async () => {
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe: vi.fn(),
      initializeShared: vi.fn().mockResolvedValue({ ...INITIALIZED, endpointId: "other" }),
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector("[data-initialize-wasapi-shared]")).not.toBeNull());
    (document.querySelector("[data-initialize-wasapi-shared]") as HTMLButtonElement).click();

    await vi.waitFor(() => expect(document.body.textContent).toContain(
      "WASAPI shared buffer could not be initialized.",
    ));
    expect(document.body.textContent).not.toContain("other");
  });

  test("refresh discards a late shared-buffer result", async () => {
    let resolve: { value?: (result: typeof INITIALIZED) => void } = {};
    const pending = new Promise<typeof INITIALIZED>((done) => { resolve.value = done; });
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    mountAudioSetup(document.body, {
      discover,
      probe: vi.fn(),
      initializeShared: vi.fn().mockReturnValue(pending),
    });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    refresh.click();
    await vi.waitFor(() => expect(document.querySelector("[data-initialize-wasapi-shared]")).not.toBeNull());
    (document.querySelector("[data-initialize-wasapi-shared]") as HTMLButtonElement).click();
    refresh.click();
    await vi.waitFor(() => expect(discover).toHaveBeenCalledTimes(2));
    resolve.value?.(INITIALIZED);
    await Promise.resolve();

    expect(document.body.textContent).not.toContain("Shared buffer initialized and released");
    expect(document.querySelector("[data-audio-announcement]")?.textContent)
      .toBe("Found 3 audio endpoints and 1 registered ASIO drivers.");
  });

  test("refresh invalidates shared-buffer evidence before discovery finishes", async () => {
    let finishRefresh: { value?: (discovery: AudioDiscovery) => void } = {};
    let finishInitialization: { value?: (result: typeof INITIALIZED) => void } = {};
    const refreshPending = new Promise<AudioDiscovery>((done) => { finishRefresh.value = done; });
    const initializationPending = new Promise<typeof INITIALIZED>((done) => {
      finishInitialization.value = done;
    });
    const discover = vi.fn()
      .mockResolvedValueOnce(DISCOVERY)
      .mockReturnValueOnce(refreshPending);
    mountAudioSetup(document.body, {
      discover,
      probe: vi.fn(),
      initializeShared: vi.fn().mockReturnValue(initializationPending),
    });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    refresh.click();
    await vi.waitFor(() => expect(document.querySelector("[data-initialize-wasapi-shared]")).not.toBeNull());
    (document.querySelector("[data-initialize-wasapi-shared]") as HTMLButtonElement).click();
    refresh.click();
    await vi.waitFor(() => expect(discover).toHaveBeenCalledTimes(2));

    finishInitialization.value?.(INITIALIZED);
    await Promise.resolve();

    expect(document.body.textContent).not.toContain("Shared buffer initialized and released");
    finishRefresh.value?.(DISCOVERY);
    await vi.waitFor(() => expect(refresh.disabled).toBe(false));
  });

  test("failed refresh invalidates old evidence but leaves the existing action usable", async () => {
    let finishInitialization: { value?: (result: typeof INITIALIZED) => void } = {};
    const initializationPending = new Promise<typeof INITIALIZED>((done) => {
      finishInitialization.value = done;
    });
    const initializeShared = vi.fn()
      .mockReturnValueOnce(initializationPending)
      .mockResolvedValueOnce(INITIALIZED);
    const discover = vi.fn()
      .mockResolvedValueOnce(DISCOVERY)
      .mockRejectedValueOnce(new Error("private refresh detail"));
    mountAudioSetup(document.body, { discover, probe: vi.fn(), initializeShared });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    refresh.click();
    await vi.waitFor(() => expect(document.querySelector("[data-initialize-wasapi-shared]")).not.toBeNull());
    const initialize = document.querySelector("[data-initialize-wasapi-shared]") as HTMLButtonElement;
    initialize.click();
    refresh.click();
    await vi.waitFor(() => expect(document.body.textContent).toContain("Audio devices could not be refreshed."));

    finishInitialization.value?.(INITIALIZED);
    await Promise.resolve();
    expect(document.body.textContent).not.toContain("Shared buffer initialized and released");

    initialize.click();
    await vi.waitFor(() => expect(document.body.textContent).toContain(
      "Shared buffer initialized and released",
    ));
    expect(initializeShared).toHaveBeenCalledTimes(2);
  });

  test("renders distinct endpoint identities and unprobed ASIO registrations", async () => {
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    mountAudioSetup(document.body, { discover, probe: vi.fn(), initializeShared: vi.fn() });
    (document.querySelector("button") as HTMLButtonElement).click();

    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(3));
    const speakers = [...document.querySelectorAll("[data-endpoint-id]")]
      .filter((item) => item.textContent?.includes("Speakers"));
    expect(speakers).toHaveLength(2);
    expect(speakers.map((item) => (item as HTMLElement).dataset.endpointId)).toEqual(["output-1", "output-2"]);
    expect(document.body.textContent).toContain("Output · Active");
    expect(document.body.textContent).toContain("Input · Active");
    expect(document.body.textContent).toContain("Rane PERFORMER ASIO · Registered, capabilities unprobed");
    expect(document.body.textContent).toContain("One stale endpoint name was unavailable");
    expect(document.body.textContent).not.toContain("6e9aac96");
    expect(document.querySelectorAll("[data-probe-wasapi]")).toHaveLength(2);
  });

  test("replaces results and announces one summary per completed refresh", async () => {
    const discover = vi.fn()
      .mockResolvedValueOnce(DISCOVERY)
      .mockResolvedValueOnce({ endpoints: [], asioDrivers: [], issues: [] });
    mountAudioSetup(document.body, { discover, probe: vi.fn(), initializeShared: vi.fn() });
    const button = document.querySelector("button") as HTMLButtonElement;
    button.focus();
    button.click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(3));
    button.click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(0));

    expect(document.querySelectorAll("[data-audio-announcement]")).toHaveLength(1);
    expect(document.querySelector("[data-audio-announcement]")?.textContent)
      .toBe("Found 0 audio endpoints and 0 registered ASIO drivers.");
    expect(document.activeElement).toBe(button);
  });

  test("restores the refresh action after a discovery failure", async () => {
    mountAudioSetup(document.body, {
      discover: vi.fn().mockRejectedValue(new Error("private detail")),
      probe: vi.fn(),
      initializeShared: vi.fn(),
    });
    const button = document.querySelector("button") as HTMLButtonElement;
    button.click();
    expect(button.disabled).toBe(true);
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(document.body.textContent).toContain("Audio devices could not be refreshed.");
    expect(document.body.textContent).not.toContain("private detail");
  });

  test("does not steal focus when the DJ moves on during refresh", async () => {
    let resolve: { value?: (discovery: AudioDiscovery) => void } = {};
    const pending = new Promise<AudioDiscovery>((done) => { resolve.value = done; });
    mountAudioSetup(document.body, {
      discover: vi.fn().mockReturnValue(pending),
      probe: vi.fn(),
      initializeShared: vi.fn(),
    });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    const other = document.createElement("button");
    other.textContent = "Other action";
    document.body.append(other);
    refresh.focus();
    refresh.click();
    other.focus();

    resolve.value?.(DISCOVERY);
    await vi.waitFor(() => expect(refresh.disabled).toBe(false));

    expect(document.activeElement).toBe(other);
  });

  test("probes only the activated endpoint and labels shared-mode evidence", async () => {
    let resolve: { value?: (capabilities: typeof CAPABILITIES) => void } = {};
    const pending = new Promise<typeof CAPABILITIES>((done) => { resolve.value = done; });
    const probe = vi.fn().mockReturnValue(pending);
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe,
      initializeShared: vi.fn(),
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-probe-wasapi]")).toHaveLength(2));
    const buttons = [...document.querySelectorAll<HTMLButtonElement>("[data-probe-wasapi]")];

    buttons[0].click();

    expect(probe).toHaveBeenCalledWith("output-1");
    expect(buttons[0].disabled).toBe(true);
    expect(buttons[1].disabled).toBe(false);
    expect(document.body.textContent).toContain("Probing WASAPI shared mix…");
    resolve.value?.(CAPABILITIES);
    await vi.waitFor(() => expect(buttons[0].disabled).toBe(false));
    expect(document.body.textContent).toContain(
      "WASAPI shared mix: 8 channels · 48,000 Hz · 32-bit. Engine periods: 10 ms default · 3 ms minimum. This exact format is supported in exclusive mode. Official exclusive rates with the same channels and sample representation: 44,100 Hz · 48,000 Hz. Shared engine periods: 48–480 frames · 4-frame steps · 480-frame default. The 128-frame target is available. Other channel layouts, sample representations, and ASIO untested. Stream initialization, actual buffer size, playback stability, and end-to-end latency untested.",
    );
    const announcement = document.querySelector("[data-audio-announcement]")?.textContent ?? "";
    expect(announcement).toBe("WASAPI shared mix ready.");
    expect(announcement).not.toContain("output-1");
  });

  test("labels an unsupported exact exclusive format without implying broader coverage", async () => {
    const unsupported = {
      endpointId: "output-1",
      channels: 8,
      sampleRateHz: 48_000,
      bitsPerSample: 32,
      defaultPeriod100ns: 100_000,
      minimumPeriod100ns: 30_000,
      exclusiveSharedMixSupported: false,
      exclusiveSharedFormatSampleRatesHz: [],
      sharedModeEnginePeriods: {
        defaultFrames: 480,
        fundamentalFrames: 32,
        minimumFrames: 160,
        maximumFrames: 480,
      },
    };
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe: vi.fn().mockResolvedValue(unsupported),
      initializeShared: vi.fn(),
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector("[data-probe-wasapi]")).not.toBeNull());
    (document.querySelector("[data-probe-wasapi]") as HTMLButtonElement).click();

    await vi.waitFor(() => expect(document.body.textContent).toContain(
      "This exact format is not supported in exclusive mode. No official exclusive rates were accepted with the same channels and sample representation.",
    ));
    expect(document.body.textContent).toContain(
      "Shared engine periods: 160–480 frames · 32-frame steps · 480-frame default. The 128-frame target is not available.",
    );
    expect(document.body.textContent).toContain(
      "Stream initialization, actual buffer size, playback stability, and end-to-end latency untested.",
    );
  });

  test("labels official-rate evidence as unavailable when attached to an older engine", async () => {
    const olderCapabilities = {
      ...CAPABILITIES,
      exclusiveSharedFormatSampleRatesHz: null,
      sharedModeEnginePeriods: null,
    };
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe: vi.fn().mockResolvedValue(olderCapabilities),
      initializeShared: vi.fn(),
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector("[data-probe-wasapi]")).not.toBeNull());
    (document.querySelector("[data-probe-wasapi]") as HTMLButtonElement).click();

    await vi.waitFor(() => expect(document.body.textContent).toContain(
      "Official exclusive-rate probing is unavailable from the attached engine.",
    ));
    expect(document.body.textContent).not.toContain("No official exclusive rates were accepted");
    expect(document.body.textContent).toContain(
      "Shared engine-period probing is unavailable from the attached engine.",
    );
    expect(document.body.textContent).toContain(
      "Stream initialization, actual buffer size, playback stability, and end-to-end latency untested.",
    );
  });

  test("refresh discards stale capabilities and late probe results", async () => {
    let resolve: { value?: (capabilities: typeof CAPABILITIES) => void } = {};
    const pending = new Promise<typeof CAPABILITIES>((done) => { resolve.value = done; });
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    mountAudioSetup(document.body, {
      discover,
      probe: vi.fn().mockReturnValue(pending),
      initializeShared: vi.fn(),
    });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    refresh.click();
    await vi.waitFor(() => expect(document.querySelector("[data-probe-wasapi]")).not.toBeNull());
    (document.querySelector("[data-probe-wasapi]") as HTMLButtonElement).click();
    refresh.click();
    await vi.waitFor(() => expect(discover).toHaveBeenCalledTimes(2));
    resolve.value?.(CAPABILITIES);
    await Promise.resolve();

    expect(document.body.textContent).not.toContain("WASAPI shared mix: 8 channels");
    expect(document.querySelector("[data-audio-announcement]")?.textContent)
      .toBe("Found 3 audio endpoints and 1 registered ASIO drivers.");
  });

  test("keeps probe failures generic", async () => {
    mountAudioSetup(document.body, {
      discover: vi.fn().mockResolvedValue(DISCOVERY),
      probe: vi.fn().mockRejectedValue(new Error("private endpoint output-1")),
      initializeShared: vi.fn(),
    });
    (document.querySelector("button") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector("[data-probe-wasapi]")).not.toBeNull());
    (document.querySelector("[data-probe-wasapi]") as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.body.textContent).toContain("WASAPI shared mix could not be read."));

    expect(document.body.textContent).not.toContain("private endpoint");
  });
});
