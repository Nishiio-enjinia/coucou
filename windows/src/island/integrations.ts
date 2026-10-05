// Integration events → island state. Port of the `handle…` methods in the Swift
// pollers: a genuinely new item flips the pill to finished/error, badges it when
// the pill isn't focused, plays a sound, and clears itself after 60 s.

import { onEvent, Bridge, type IntegrationUpdate } from "../core/bridge";
import { Sound } from "../core/sound";
import { State, type AgentTask } from "../core/state";
import type { Island } from "./island";

/** Which Credential Manager key backs each pill. */
const KEY_FOR: Record<string, string> = {
  integration_stripe: "stripe-api-key",
  integration_github: "github-token",
  integration_gitlab: "gitlab-token",
  integration_jenkins: "jenkins-token",
  integration_vercel: "vercel-token",
  integration_n8n: "n8n-api-key",
  integration_resend: "resend-api-key",
  integration_notion: "notion-api-key",
  integration_calcom: "calcom-api-key",
};

const clearTimers = new Map<string, number>();

export function registerIntegrationHandlers(island: Island) {
  void onEvent<IntegrationUpdate>("integration", (update) => handle(island, update));
  void refreshConfigured();
}

/** Asks Rust which keys exist so the idle cards can say so. */
export async function refreshConfigured() {
  for (const [id, key] of Object.entries(KEY_FOR)) {
    const present = (await Bridge.secretPresent(key)) ?? false;
    const info = State.integrations[id] ?? { data: {}, error: null, loaded: false, configured: false };
    State.integrations[id] = { ...info, configured: present };
  }
  const gitlab = State.integrations.integration_gitlab;
  if (gitlab) {
    const url = (await Bridge.secretPresent("gitlab-url")) ?? false;
    gitlab.configured = gitlab.configured && url;
  }
  const jenkins = State.integrations.integration_jenkins;
  if (jenkins) {
    const url = (await Bridge.secretPresent("jenkins-url")) ?? false;
    const user = (await Bridge.secretPresent("jenkins-user")) ?? false;
    jenkins.configured = jenkins.configured && url && user;
  }
  const hooks = State.settings.hooksInstalled;
  const claude = State.integrations.integration_claude ?? {
    data: {}, error: null, loaded: false, configured: false,
  };
  State.integrations.integration_claude = { ...claude, configured: hooks };
  const rows = (await Bridge.idesStatus()) ?? [];
  const cursorHooks = rows.find((row) => row.id === "cursor")?.installed ?? false;
  const cursor = State.integrations.agent_cursor ?? {
    data: {}, error: null, loaded: false, configured: false,
  };
  State.integrations.agent_cursor = { ...cursor, configured: cursorHooks };
  State.notify();
}

function handle(island: Island, update: IntegrationUpdate) {
  if (State.paused) return;

  const previous = State.integrations[update.id];
  State.integrations[update.id] = {
    data: update.error ? (previous?.data ?? {}) : update.data,
    error: update.error,
    loaded: update.error ? (previous?.loaded ?? false) : true,
    configured: previous?.configured ?? true,
  };

  const event = update.event;
  if (event) {
    const task = State.tasks.find((t) => t.id === update.id);
    if (task) {
      const phase =
        event.phase === "working" || event.phase === "finished" || event.phase === "error"
          ? event.phase
          : event.success
            ? "finished"
            : "error";
      const success = phase === "finished";
      task.state = phase === "working" ? "working" : success ? "finished" : "error";
      task.steps = event.detail ? [event.label, event.detail] : [event.label];
      task.stepIndex = task.steps.length - 1;
      if (phase !== "working" && State.focusId !== update.id) {
        task.pillBadge = success ? "finished" : "error";
      }
      Sound.play(phase === "working" ? "work" : success ? "finish" : "error");
      // Same as the Swift pollers: show the compact island so the badge is seen,
      // but never steal the screen for a successful deploy.
      island.reveal();

      if (phase !== "working") {
        const existing = clearTimers.get(update.id);
        if (existing != null) window.clearTimeout(existing);
        clearTimers.set(
          update.id,
          window.setTimeout(() => {
            clearTimers.delete(update.id);
            const t = State.tasks.find((x) => x.id === update.id);
            if (!t || (t.state !== "finished" && t.state !== "error")) return;
            t.state = "idle";
            t.steps = [];
            t.stepIndex = 0;
            t.pillBadge = null;
            State.notify();
          }, 60_000),
        );
      }
    }
  }

  if (update.id === "integration_jenkins" && !update.error) {
    const task = State.tasks.find((t) => t.id === update.id);
    if (task) syncJenkins(task, update.data);
  }

  State.notify();
}

/** Keeps the Jenkins pill on the live builds between alerts. A sound fires only on a transition. */
function syncJenkins(task: AgentTask, data: Record<string, unknown>) {
  const builds = Array.isArray(data.builds) ? (data.builds as Record<string, unknown>[]) : [];
  const steps = builds
    .filter((build) => build.phase === "building" || build.phase === "queued")
    .slice(0, 3)
    .map((build) => {
      const name = String(build.name ?? "Jenkins");
      const number = typeof build.number === "number" ? ` #${build.number}` : "";
      return `${name}${number}`;
    });
  if (steps.length > 0) {
    task.state = "working";
    task.steps = steps;
    task.stepIndex = 0;
    return;
  }
  if (task.state === "working") {
    task.state = "idle";
    task.steps = [];
    task.stepIndex = 0;
  }
}
