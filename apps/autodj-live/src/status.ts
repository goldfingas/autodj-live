export type FoundationState =
  | "starting"
  | "ready"
  | "degraded"
  | "reattaching"
  | "stopped";
export type Component = "engine" | "sidecar";
export type ComponentState = "starting" | "ready" | "failed" | "reattaching" | "stopped";

export interface FoundationStatus {
  state: FoundationState;
  engine: ComponentState;
  sidecar: ComponentState;
  uptimeSeconds: number;
}

export interface FoundationApi {
  getStatus(): Promise<FoundationStatus>;
  retry(component: Component): Promise<FoundationStatus>;
  shutdown(): Promise<void>;
}

export function announceStatus(region: HTMLElement, message: string): boolean {
  if (region.textContent === message) return false;
  region.textContent = message;
  return true;
}

const DISPLAY: Record<ComponentState, string> = {
  starting: "Starting",
  ready: "Ready",
  failed: "Needs attention",
  reattaching: "Reattaching",
  stopped: "Stopped",
};

function summary(status: FoundationStatus): string {
  if (status.state === "degraded") {
    const failed = status.engine === "failed" ? "Engine" : "Sidecar";
    return `${failed} needs attention.`;
  }
  const messages: Record<Exclude<FoundationState, "degraded">, string> = {
    starting: "AutoDJ Live is starting.",
    ready: "AutoDJ Live is ready.",
    reattaching: "AutoDJ Live is reattaching.",
    stopped: "AutoDJ Live is stopped.",
  };
  return messages[status.state];
}

function componentRow(component: Component): { row: HTMLDivElement; value: HTMLElement } {
  const row = document.createElement("div");
  row.dataset.component = component;
  const term = document.createElement("dt");
  term.textContent = component === "engine" ? "Engine" : "Sidecar";
  const value = document.createElement("dd");
  row.append(term, value);
  return { row, value };
}

export function mountStatusSurface(root: HTMLElement, api: FoundationApi) {
  const heading = document.createElement("h1");
  heading.textContent = "AutoDJ Live";
  const intro = document.createElement("p");
  intro.textContent = "Foundation services";
  const overall = document.createElement("p");
  overall.className = "overall";
  overall.tabIndex = -1;
  const list = document.createElement("dl");
  const engine = componentRow("engine");
  const sidecar = componentRow("sidecar");
  list.append(engine.row, sidecar.row);
  const uptimeLabel = document.createElement("p");
  uptimeLabel.append("Uptime: ");
  const uptime = document.createElement("span");
  uptime.dataset.uptime = "";
  uptime.setAttribute("aria-live", "off");
  uptimeLabel.append(uptime);
  const retry = document.createElement("button");
  retry.type = "button";
  retry.hidden = true;
  const shutdown = document.createElement("button");
  shutdown.type = "button";
  shutdown.textContent = "Stop AutoDJ Live";
  const actions = document.createElement("div");
  actions.className = "actions";
  actions.append(retry, shutdown);
  const announcement = document.createElement("p");
  announcement.setAttribute("role", "status");
  announcement.setAttribute("aria-live", "polite");
  announcement.className = "visually-hidden";
  root.replaceChildren(heading, intro, overall, list, uptimeLabel, actions, announcement);

  let retryTarget: Component | undefined;

  function setUptime(seconds: number) {
    uptime.textContent = `${Math.max(0, Math.floor(seconds))} seconds`;
  }

  function render(status: FoundationStatus) {
    overall.textContent = summary(status).replace(/\.$/, "");
    engine.value.textContent = DISPLAY[status.engine];
    sidecar.value.textContent = DISPLAY[status.sidecar];
    setUptime(status.uptimeSeconds);
    retryTarget = status.engine === "failed" ? "engine" : status.sidecar === "failed" ? "sidecar" : undefined;
    retry.hidden = retryTarget === undefined;
    if (retryTarget) retry.textContent = `Retry ${retryTarget}`;
    shutdown.disabled = status.state === "stopped";
    announceStatus(announcement, summary(status));
  }

  async function refresh() {
    render(await api.getStatus());
  }

  retry.addEventListener("click", async () => {
    const target = retryTarget;
    if (!target) return;
    const focused = document.activeElement === retry;
    render(await api.retry(target));
    if (focused) overall.focus();
  });
  shutdown.addEventListener("click", async () => {
    await api.shutdown();
    await refresh();
  });

  render({
    state: "starting",
    engine: "starting",
    sidecar: "starting",
    uptimeSeconds: 0,
  });

  return { refresh, render, setUptime };
}
