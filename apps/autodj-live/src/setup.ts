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
}

export interface AudioSetupApi {
  discover(): Promise<AudioDiscovery>;
  probe(endpointId: string): Promise<WasapiCapabilities>;
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
  return `WASAPI shared mix: ${capabilities.channels} channels · ${number.format(capabilities.sampleRateHz)} Hz · ${capabilities.bitsPerSample}-bit. Engine periods: ${number.format(capabilities.defaultPeriod100ns / 10_000)} ms default · ${number.format(capabilities.minimumPeriod100ns / 10_000)} ms minimum. This exact format is ${exactExclusive} in exclusive mode. Other exclusive formats and ASIO untested.`;
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
  explanation.textContent = "Read-only device inventory. Nothing is selected or opened.";
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
    const currentGeneration = generation;
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
