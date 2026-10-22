export type AudioDirection = "input" | "output";
export type AudioEndpointState =
  | "active"
  | "disabled"
  | "not_present"
  | "unplugged"
  | { unknown: number };

export interface AudioEndpoint {
  id: string;
  name: string;
  direction: AudioDirection;
  state: AudioEndpointState;
}

export interface AudioDiscovery {
  endpoints: AudioEndpoint[];
  asioDrivers: Array<{ registration: string; clsid: string }>;
  issues: string[];
}

export interface WasapiCapabilities {
  endpointId: string;
  channels: number;
  sampleRateHz: number;
  bitsPerSample: number;
  defaultPeriod100ns: number;
  minimumPeriod100ns: number;
  exclusiveSharedMixSupported: boolean;
  exclusiveSharedFormatSampleRatesHz: number[] | null;
  sharedModeEnginePeriods: {
    defaultFrames: number;
    fundamentalFrames: number;
    minimumFrames: number;
    maximumFrames: number;
  } | null;
}

export interface WasapiSharedInitialization {
  endpointId: string;
  requestedPeriodFrames: number;
  bufferFrames: number;
}

export interface AudioSetupApi {
  discover(): Promise<AudioDiscovery>;
  probe(endpointId: string): Promise<WasapiCapabilities>;
  initializeShared(endpointId: string): Promise<WasapiSharedInitialization>;
}

function label(value: string): string {
  return value.replace("_", " ").replace(/^./, (letter) => letter.toUpperCase());
}

function stateLabel(state: AudioEndpointState): string {
  return typeof state === "string" ? label(state) : `Unknown (${state.unknown})`;
}

const number = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

function capabilityLabel(capabilities: WasapiCapabilities): string {
  const exactExclusive = capabilities.exclusiveSharedMixSupported ? "supported" : "not supported";
  const probedRates = capabilities.exclusiveSharedFormatSampleRatesHz;
  const acceptedRates = probedRates === null
    ? "Official exclusive-rate probing is unavailable from the attached engine."
    : probedRates.length > 0
      ? `Official exclusive rates with the same channels and sample representation: ${probedRates.map((rate) => `${number.format(rate)} Hz`).join(" · ")}.`
      : "No official exclusive rates were accepted with the same channels and sample representation.";
  const periods = capabilities.sharedModeEnginePeriods;
  const periodEvidence = periods === null
    ? "Shared engine-period probing is unavailable from the attached engine."
    : `Shared engine periods: ${number.format(periods.minimumFrames)}–${number.format(periods.maximumFrames)} frames · ${number.format(periods.fundamentalFrames)}-frame steps · ${number.format(periods.defaultFrames)}-frame default. The 128-frame target is ${128 >= periods.minimumFrames && 128 <= periods.maximumFrames && 128 % periods.fundamentalFrames === 0 ? "available" : "not available"}.`;
  return `WASAPI shared mix: ${capabilities.channels} channels · ${number.format(capabilities.sampleRateHz)} Hz · ${capabilities.bitsPerSample}-bit. Engine periods: ${number.format(capabilities.defaultPeriod100ns / 10_000)} ms default · ${number.format(capabilities.minimumPeriod100ns / 10_000)} ms minimum. This exact format is ${exactExclusive} in exclusive mode. ${acceptedRates} ${periodEvidence} Other channel layouts, sample representations, and ASIO untested. Stream initialization, actual buffer size, playback stability, and end-to-end latency untested.`;
}

function list(headingText: string): { container: HTMLElement; items: HTMLUListElement } {
  const container = document.createElement("section");
  const heading = document.createElement("h3");
  heading.textContent = headingText;
  const items = document.createElement("ul");
  container.append(heading, items);
  return { container, items };
}

export function mountAudioSetup(root: HTMLElement, api: AudioSetupApi) {
  let generation = 0;
  const section = document.createElement("section");
  section.className = "audio-setup";
  const heading = document.createElement("h2");
  heading.textContent = "Audio setup";
  const explanation = document.createElement("p");
  explanation.textContent = "Device inventory and explicit checks. Nothing is selected. Checking a shared buffer briefly initializes and releases it. Audio is never started.";
  const refresh = document.createElement("button");
  refresh.type = "button";
  refresh.textContent = "Refresh audio devices";
  const outcome = document.createElement("p");
  outcome.dataset.audioOutcome = "";
  const endpoints = list("Windows audio endpoints");
  const asio = list("ASIO drivers");
  const issues = list("Discovery notes");
  const announcement = document.createElement("p");
  announcement.dataset.audioAnnouncement = "";
  announcement.className = "visually-hidden";
  announcement.setAttribute("role", "status");
  announcement.setAttribute("aria-live", "polite");
  section.append(
    heading,
    explanation,
    refresh,
    outcome,
    endpoints.container,
    asio.container,
    issues.container,
    announcement,
  );
  root.append(section);

  function render(discovery: AudioDiscovery) {
    generation += 1;
    endpoints.items.replaceChildren(...discovery.endpoints.map((endpoint) => {
      const item = document.createElement("li");
      item.dataset.endpointId = endpoint.id;
      const identity = document.createElement("span");
      identity.textContent = `${endpoint.name} — ${label(endpoint.direction)} · ${stateLabel(endpoint.state)}`;
      item.append(identity);
      if (endpoint.state === "active") {
        const probe = document.createElement("button");
        probe.type = "button";
        probe.dataset.probeWasapi = "";
        probe.textContent = "Probe WASAPI";
        probe.setAttribute("aria-label", `Probe WASAPI shared mix for ${endpoint.name}`);
        const result = document.createElement("p");
        result.className = "probe-result";
        probe.addEventListener("click", async () => {
          const currentGeneration = generation;
          const restoreFocus = document.activeElement === probe;
          probe.disabled = true;
          result.textContent = "Probing WASAPI shared mix…";
          try {
            const capabilities = await api.probe(endpoint.id);
            if (generation !== currentGeneration) return;
            if (capabilities.endpointId !== endpoint.id) throw new Error("endpoint mismatch");
            result.textContent = capabilityLabel(capabilities);
            announcement.textContent = "WASAPI shared mix ready.";
          } catch {
            if (generation !== currentGeneration) return;
            result.textContent = "WASAPI shared mix could not be read.";
            announcement.textContent = result.textContent;
          } finally {
            probe.disabled = false;
            if (restoreFocus && (document.activeElement === probe || document.activeElement === document.body)) {
              probe.focus();
            }
          }
        });
        item.append(probe, result);
        const initialize = document.createElement("button");
        initialize.type = "button";
        initialize.dataset.initializeWasapiShared = "";
        initialize.textContent = "Check shared buffer";
        initialize.setAttribute("aria-label", `Check WASAPI shared buffer for ${endpoint.name}`);
        const initializationResult = document.createElement("p");
        initializationResult.className = "probe-result";
        initialize.addEventListener("click", async () => {
          const currentGeneration = generation;
          const restoreFocus = document.activeElement === initialize;
          initialize.disabled = true;
          initializationResult.textContent = "Initializing and releasing WASAPI shared bufferâ€¦";
          try {
            const initialized = await api.initializeShared(endpoint.id);
            if (generation !== currentGeneration) return;
            if (
              initialized.endpointId !== endpoint.id
              || initialized.requestedPeriodFrames <= 0
              || initialized.bufferFrames <= 0
            ) throw new Error("invalid initialization evidence");
            initializationResult.textContent = `Shared buffer initialized and released: requested ${number.format(initialized.requestedPeriodFrames)} frames Â· allocated ${number.format(initialized.bufferFrames)} frames. The stream was never started; playback stability and latency remain untested.`;
            announcement.textContent = "WASAPI shared buffer check ready.";
          } catch {
            if (generation !== currentGeneration) return;
            initializationResult.textContent = "WASAPI shared buffer could not be initialized.";
            announcement.textContent = initializationResult.textContent;
          } finally {
            initialize.disabled = false;
            if (restoreFocus && (document.activeElement === initialize || document.activeElement === document.body)) {
              initialize.focus();
            }
          }
        });
        item.append(initialize, initializationResult);
      }
      return item;
    }));
    asio.items.replaceChildren(...discovery.asioDrivers.map((driver) => {
      const item = document.createElement("li");
      item.textContent = `${driver.registration} · Registered, capabilities unprobed`;
      return item;
    }));
    issues.items.replaceChildren(...discovery.issues.map((issue) => {
      const item = document.createElement("li");
      item.textContent = issue;
      return item;
    }));
    const summary = `Found ${discovery.endpoints.length} audio endpoints and ${discovery.asioDrivers.length} registered ASIO drivers.`;
    outcome.textContent = summary;
    announcement.textContent = summary;
  }

  refresh.addEventListener("click", async () => {
    generation += 1;
    const restoreFocus = document.activeElement === refresh;
    refresh.disabled = true;
    outcome.textContent = "Refreshing audio devices…";
    try {
      render(await api.discover());
    } catch {
      outcome.textContent = "Audio devices could not be refreshed.";
      announcement.textContent = outcome.textContent;
    } finally {
      refresh.disabled = false;
      if (restoreFocus && (document.activeElement === refresh || document.activeElement === document.body)) {
        refresh.focus();
      }
    }
  });

  return { render };
}
