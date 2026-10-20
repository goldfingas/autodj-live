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

export interface AudioSetupApi {
  discover(): Promise<AudioDiscovery>;
}

function label(value: string): string {
  return value.replace("_", " ").replace(/^./, (letter) => letter.toUpperCase());
}

function stateLabel(state: AudioEndpointState): string {
  return typeof state === "string" ? label(state) : `Unknown (${state.unknown})`;
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
    endpoints.items.replaceChildren(...discovery.endpoints.map((endpoint) => {
      const item = document.createElement("li");
      item.dataset.endpointId = endpoint.id;
      item.textContent = `${endpoint.name} — ${label(endpoint.direction)} · ${stateLabel(endpoint.state)}`;
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
