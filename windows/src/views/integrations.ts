// Integration cards shown in the overview's left card — DOM ports of
// IntegrationCardView and friends from IslandViewContent.swift.
//
// Cal.com is the one simplification: macOS shows a three-level calendar
// (month → day → booking); here it is the list of upcoming bookings.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { t } from "../core/i18n";
import { State, type AgentTask } from "../core/state";
import { Bridge } from "../core/bridge";

/** Same shape as the Swift `timeAgo` computed properties. */
export function timeAgo(value: unknown): string {
  const date = typeof value === "number" ? new Date(value) : new Date(String(value));
  const diff = (Date.now() - date.getTime()) / 1000;
  if (!Number.isFinite(diff)) return "";
  if (diff < 60) return t("time.now");
  if (diff < 3600) return t("time.m", { n: Math.floor(diff / 60) });
  if (diff < 86400) return t("time.h", { n: Math.floor(diff / 3600) });
  return t("time.d", { n: Math.floor(diff / 86400) });
}

function header(color: string, name: string, kind: string, extra?: Node): HTMLElement {
  const row = h("div", { class: "int-head" }, dot(color, 7), h("b", { text: name }), h("span", { text: kind }));
  if (extra) row.append(extra);
  return row;
}

/** Highlighted first row + plain rows, the layout every list card shares. */
function listRow(accent: string, first: boolean, ...children: Node[]): HTMLElement {
  const row = h("div", { class: first ? "int-row first" : "int-row" }, dot(accent, 5), ...children);
  if (first) row.style.background = `${accent}14`;
  return row;
}

function get(id: string): Record<string, unknown> {
  return (State.integrations[id]?.data ?? {}) as Record<string, unknown>;
}

function arr(id: string, key: string): Record<string, unknown>[] {
  const v = get(id)[key];
  return Array.isArray(v) ? (v as Record<string, unknown>[]) : [];
}

// ── Not configured / idle ─────────────────────────────────────────────────────

const OPEN_URLS: Record<string, string> = {
  integration_resend: "https://resend.com/emails",
  integration_vercel: "https://vercel.com/dashboard",
  integration_github: "https://github.com",
  integration_stripe: "https://dashboard.stripe.com/payments",
  integration_notion: "https://notion.so",
  integration_calcom: "https://app.cal.com/bookings",
};

function idleCard(task: AgentTask, openSettings: () => void): HTMLElement {
  const info = State.integrations[task.id];
  const configured = info?.configured ?? false;
  const error = info?.error ?? null;
  // The Claude Code pill is about hooks, not a key — the macOS wording would be
  // misleading here.
  const workspace = task.id === "integration_claude" || task.id === "agent_cursor";
  const missing = workspace ? t("int.hooksMissing") : t("int.keyMissing");
  const label = error ?? (configured ? t("int.loading") : missing);
  const statusColor = error || !configured ? "#F4505E" : "#22C55E";

  const actions = h("div", { class: "int-actions" });
  if (task.id === "integration_claude") {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}b3`,
        text: t("int.openVSCode"),
        onclick: () => void Bridge.openInVSCode(task.sessionCwd ?? null),
      }),
    );
  } else if (task.id === "agent_cursor") {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}b3`,
        text: t("int.openCursor"),
        onclick: () => void Bridge.openCursor(task.sessionCwd ?? null),
      }),
    );
  } else if (task.id === "integration_n8n") {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("int.openN8n"),
        onclick: () => void Bridge.openN8n(),
      }),
    );
  } else if (OPEN_URLS[task.id]) {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("int.open", { name: task.name }),
        onclick: () => void Bridge.openUrl(OPEN_URLS[task.id]),
      }),
    );
  }
  if (configured) {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("int.refresh"),
        onclick: () => void Bridge.refreshIntegration(task.id),
      }),
    );
  } else {
    actions.append(
      h("button", { class: "link-btn", style: "color:#8e939c", text: t("settings.open"), onclick: openSettings }),
    );
  }

  return h(
    "div",
    { class: "int-card" },
    header(task.color, task.id === "integration_claude" ? "VS Code" : task.name, t("int.kind")),
    h("div", { class: "int-status" }, dot(statusColor, 5), h("span", { text: label })),
    actions,
  );
}

// ── Vercel ────────────────────────────────────────────────────────────────────

function vercelCard(onDetail: () => void): HTMLElement {
  const deployments = arr("integration_vercel", "deployments");
  const rows = h("div", { class: "int-rows" });
  deployments.slice(0, 3).forEach((d, i) => {
    const accent = d.state === "READY" ? "#22C55E" : "#F4505E";
    const name = h("span", { class: "int-name", text: String(d.projectName ?? "") });
    const ago = h("span", { class: "int-ago", text: timeAgo(d.createdAt) });
    if (i === 0) {
      const more = h(
        "button",
        { class: "int-more", title: t("int.details"), onclick: onDetail },
        svg(ICONS.ellipsis, 8),
      );
      rows.append(listRow(accent, true, name, ago, more));
    } else {
      rows.append(listRow(accent, false, name, ago));
    }
  });
  return h("div", { class: "int-card" }, header("#7C5CFF", "Vercel", t("int.deployments")), rows);
}

function vercelDetail(onBack: () => void): HTMLElement {
  const d = arr("integration_vercel", "deployments")[0] ?? {};
  const success = d.state === "READY";
  const accent = success ? "#22C55E" : "#F4505E";
  const status = success ? t("int.ready") : d.state === "CANCELED" ? t("int.canceled") : t("int.error");
  const body = h("div", { class: "int-detail-body" });
  if (d.commitMessage) body.append(h("div", { class: "int-commit", text: String(d.commitMessage) }));
  const meta = h("div", { class: "int-meta" });
  if (d.branch) meta.append(h("span", { text: String(d.branch) }));
  const when = timeAgo(d.createdAt);
  meta.append(h("span", { text: when === t("time.now") ? when : t("time.ago", { time: when }) }));
  body.append(meta);
  if (d.url) {
    body.append(
      h("button", {
        class: "int-link",
        text: String(d.url),
        onclick: () => void Bridge.openUrl(`https://${d.url}`),
      }),
    );
  }
  return h(
    "div",
    { class: "int-card detail" },
    h(
      "div",
      { class: "int-detail-head" },
      h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })),
      dot(accent, 6),
      h("b", { text: String(d.projectName ?? t("int.deployment")) }),
      h("span", { class: "int-badge", style: `color:${accent};background:${accent}24`, text: status }),
    ),
    body,
  );
}

// ── Resend ────────────────────────────────────────────────────────────────────

function resendCard(): HTMLElement {
  const emails = arr("integration_resend", "emails");
  const total = get("integration_resend").total;
  const extra =
    total != null
      ? h("span", { class: "int-total" }, h("i", { class: "pulse" }), h("span", { text: String(total) }))
      : undefined;
  const rows = h("div", { class: "int-rows" });
  emails.slice(0, 3).forEach((e, i) => {
    const delivered = e.lastEvent === "delivered";
    const accent = delivered ? "#22C55E" : "#F4505E";
    const to = Array.isArray(e.to) ? String(e.to[0] ?? "?") : "?";
    const short = to.split("@")[0];
    const cells: Node[] = [
      h("span", { class: "int-name", text: short }),
      h("span", { class: "int-ago", text: timeAgo(e.createdAt) }),
    ];
    if (i === 0 && e.subject) cells.push(h("span", { class: "int-sub", text: String(e.subject) }));
    rows.append(listRow(accent, i === 0, ...cells));
  });
  return h("div", { class: "int-card" }, header("#22C55E", "Resend", t("int.emails"), extra), rows);
}

// ── GitHub ────────────────────────────────────────────────────────────────────

function statRow(icon: string, color: string, label: string, value: string): HTMLElement {
  return h(
    "div",
    { class: "int-stat" },
    h("i", { class: "int-stat-icon", style: `color:${color}` }, svg(icon, 10)),
    h("span", { class: "int-stat-label", text: label }),
    h("span", { class: "int-stat-value", text: value }),
  );
}

function githubCard(): HTMLElement {
  const d = get("integration_github");
  const stars = Number(d.totalStars ?? 0);
  const repos = Number(d.totalRepos ?? 0);
  const fmt = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));
  return h(
    "div",
    { class: "int-card" },
    header("#F4505E", "GitHub", t("int.overview")),
    h(
      "div",
      { class: "int-stats" },
      statRow(ICONS.star, "#F5A524", t("int.stars"), fmt(stars)),
      statRow(ICONS.stack, "#6B7079", t("int.repos"), String(repos)),
    ),
  );
}

const JENKINS_PHASE: Record<string, string> = {
  building: "#F29B38",
  queued: "#8C8C8C",
  success: "#22C55E",
  failure: "#F4505E",
  unstable: "#F5A524",
  aborted: "#6B7079",
};

function jenkinsPhaseLabel(phase: string): string {
  const key = `jenkins.${phase}`;
  const label = t(key);
  return label === key ? phase : label;
}

function jenkinsCard(): HTMLElement {
  const info = get("integration_jenkins");
  const builds = arr("integration_jenkins", "builds");
  const running = Number(info.running ?? 0);
  const kind = running > 0 ? t("jenkins.live", { n: running }) : t("jenkins.kind");
  const rows = h("div", { class: "int-rows tight" });
  if (builds.length === 0) {
    rows.append(h("div", { class: "int-empty", text: t("jenkins.empty") }));
  }
  builds.slice(0, 4).forEach((build, i) => {
    const phase = String(build.phase ?? "");
    const accent = JENKINS_PHASE[phase] ?? "#D33833";
    const number = typeof build.number === "number" ? ` #${build.number}` : "";
    const row = listRow(
      accent,
      i === 0,
      h("span", { class: "int-name", text: `${String(build.name ?? "")}${number}` }),
      h("span", { class: "int-ago", style: `color:${accent}`, text: jenkinsPhaseLabel(phase) }),
      h("span", { class: "int-ago", text: timeAgo(build.timestamp) }),
    );
    const url = build.url;
    if (typeof url === "string" && (url.startsWith("https://") || url.startsWith("http://"))) {
      row.style.cursor = "pointer";
      row.addEventListener("click", () => void Bridge.openUrl(url));
    }
    rows.append(row);
  });
  return h("div", { class: "int-card" }, header("#D33833", "Jenkins", kind), rows);
}

function gitlabLabel(event: Record<string, unknown>): string {
  const project = String(event.project ?? "").split("/").filter(Boolean).pop() ?? "";
  const title = String(event.title ?? "").trim();
  const kind = String(event.kind ?? "");
  const iid = typeof event.iid === "number" ? String(event.iid) : "";
  const ref = String(event.ref ?? "");
  const head =
    kind === "issue" && iid ? `${project}#${iid}`
    : kind === "merge" && iid ? `${project}!${iid}`
    : ref ? `${project} ${ref}`.trim()
    : project;
  return [head, title].filter(Boolean).join(" · ") || t("gitlab.event");
}

function gitlabCard(onBrowse: () => void): HTMLElement {
  const events = arr("integration_gitlab", "events");
  const rows = h("div", { class: "int-rows" });
  if (events.length === 0) {
    rows.append(h("div", { class: "int-empty", text: t("gitlab.empty") }));
  }
  events.slice(0, 3).forEach((event, i) => {
    const accent = event.failure ? "#F4505E" : "#E24329";
    const row = listRow(
      accent,
      i === 0,
      h("span", { class: "int-name", text: gitlabLabel(event) }),
      h("span", { class: "int-ago", text: timeAgo(event.createdAt) }),
    );
    const url = event.url;
    if (typeof url === "string" && (url.startsWith("https://") || url.startsWith("http://"))) {
      row.style.cursor = "pointer";
      row.addEventListener("click", () => void Bridge.openUrl(url));
    }
    rows.append(row);
  });
  const browse = h(
    "button",
    {
      class: "int-more",
      title: t("gitlab.browse"),
      onclick: (event) => {
        event.stopPropagation();
        onBrowse();
      },
    },
    svg(ICONS.chevronRight, 10, { stroke: 2.4 }),
  );
  browse.style.marginLeft = "auto";
  const head = header("#E24329", "GitLab", t("gitlab.kind"), browse);
  head.title = t("gitlab.browse");
  head.style.cursor = "pointer";
  head.addEventListener("click", onBrowse);
  return h("div", { class: "int-card" }, head, rows);
}

// ── Stripe ────────────────────────────────────────────────────────────────────

function stripeCard(): HTMLElement {
  const d = get("integration_stripe");
  const balance = (Number(d.balance ?? 0) / 100).toFixed(2);
  const currency = String(d.currency ?? "eur").toUpperCase();
  const rows = h("div", { class: "int-rows tight" });
  for (const p of arr("integration_stripe", "payments")) {
    const success = p.status === "succeeded";
    const accent = success ? "#22C55E" : "#F4505E";
    rows.append(
      h(
        "div",
        { class: "int-row" },
        dot(accent, 5),
        h("span", { class: "int-name", text: String(p.description ?? t("int.payment")) }),
        h("span", {
          class: "int-amount",
          style: "color:#22c55e",
          text: `+${(Number(p.amount ?? 0) / 100).toFixed(2)}`,
        }),
        h("span", { class: "int-ago", text: timeAgo(p.createdAt) }),
      ),
    );
  }
  return h(
    "div",
    { class: "int-card" },
    header("#0570DE", "Stripe", t("int.payments")),
    h("div", { class: "int-balance" }, h("span", { text: balance }), h("i", { text: currency })),
    rows,
  );
}

// ── Notion ────────────────────────────────────────────────────────────────────

function notionCard(): HTMLElement {
  const rows = h("div", { class: "int-rows tight" });
  for (const p of arr("integration_notion", "pages").slice(0, 3)) {
    rows.append(
      h(
        "button",
        {
          class: "int-page",
          onclick: () => {
            if (typeof p.url === "string") void Bridge.openUrl(p.url);
          },
        },
        p.emoji
          ? h("span", { class: "int-emoji", text: String(p.emoji) })
          : h("i", { class: "int-emoji" }, svg(ICONS.doc, 9)),
        h("span", { class: "int-name", text: String(p.title ?? t("int.untitled")) }),
        h("span", { class: "int-ago", text: timeAgo(p.lastEditedAt) }),
      ),
    );
  }
  return h("div", { class: "int-card" }, header("#E8E8E8", "Notion", t("int.recent")), rows);
}

// ── Cal.com ───────────────────────────────────────────────────────────────────

function calcomCard(): HTMLElement {
  const bookings = arr("integration_calcom", "bookings")
    .slice()
    .sort((a, b) => new Date(String(a.start)).getTime() - new Date(String(b.start)).getTime());
  const rows = h("div", { class: "int-rows tight" });
  if (bookings.length === 0) {
    rows.append(h("div", { class: "int-empty", text: t("int.noCalls") }));
  }
  for (const b of bookings.slice(0, 3)) {
    const when = new Date(String(b.start));
    const day = when.toLocaleDateString(undefined, { day: "2-digit", month: "2-digit" });
    const time = when.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    rows.append(
      h(
        "div",
        { class: "int-row" },
        dot("#C9956A", 4),
        h("span", { class: "int-time", text: `${day} ${time}` }),
        h("span", { class: "int-name", text: String(b.title ?? t("int.meeting")) }),
      ),
    );
  }
  return h("div", { class: "int-card" }, header("#C9956A", "Cal.com", t("int.schedule")), rows);
}

// ── n8n ───────────────────────────────────────────────────────────────────────

function n8nCard(task: AgentTask, onDetail: () => void, openSettings: () => void): HTMLElement {
  const hasActivity = task.steps.length > 0 && (task.state === "finished" || task.state === "error");
  if (!hasActivity) return idleCard(task, openSettings);
  const success = task.state === "finished";
  const accent = success ? "#22C55E" : "#F4505E";
  return h(
    "div",
    { class: "int-card" },
    header("#F29B38", "n8n", t("int.workflow")),
    h(
      "div",
      { class: "int-actions" },
      h(
        "button",
        {
          class: "int-pill",
          style: `background:${accent}1a;border-color:${accent}38`,
          onclick: onDetail,
        },
        dot(accent, 5),
        h("span", { class: "int-name", text: task.steps[0] ?? t("int.workflow") }),
        svg(ICONS.ellipsis, 8),
      ),
    ),
  );
}

function n8nDetail(task: AgentTask, onBack: () => void): HTMLElement {
  const success = task.state === "finished";
  const accent = success ? "#22C55E" : "#F4505E";
  const detail = task.steps[1];
  return h(
    "div",
    { class: "int-card detail" },
    h(
      "div",
      { class: "int-detail-head" },
      h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })),
      dot(accent, 6),
      h("b", { text: task.steps[0] ?? t("int.workflow") }),
      h("span", {
        class: "int-badge",
        style: `color:${accent};background:${accent}24`,
        text: success ? t("int.success") : t("int.failed"),
      }),
    ),
    detail
      ? h("pre", { class: "int-detail-text", text: detail })
      : h("div", {
          class: "int-status",
          text: success ? t("int.completed") : t("int.noError"),
        }),
  );
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

export interface IntegrationCardHooks {
  detailOpen: boolean;
  openDetail(): void;
  closeDetail(): void;
  openSettings(): void;
  openGitlab(): void;
}

/** True when this integration has data worth showing instead of the idle card. */
export function hasIntegrationData(id: string): boolean {
  const info = State.integrations[id];
  if (!info || info.error) return false;
  switch (id) {
    case "integration_vercel":
      return arr(id, "deployments").length > 0;
    case "integration_resend":
      return arr(id, "emails").length > 0;
    case "integration_github":
      return get(id).totalRepos != null;
    case "integration_gitlab":
      return info.loaded;
    case "integration_jenkins":
      return info.loaded;
    case "integration_stripe":
      return info.loaded;
    case "integration_notion":
      return arr(id, "pages").length > 0;
    case "integration_calcom":
      return info.loaded;
    default:
      return false;
  }
}

export function renderIntegrationCard(task: AgentTask, hooks: IntegrationCardHooks): HTMLElement {
  if (task.id === "integration_n8n") {
    const hasActivity = task.steps.length > 0 && (task.state === "finished" || task.state === "error");
    return hooks.detailOpen && hasActivity
      ? n8nDetail(task, hooks.closeDetail)
      : n8nCard(task, hooks.openDetail, hooks.openSettings);
  }
  if (task.id === "integration_vercel" && hasIntegrationData(task.id)) {
    return hooks.detailOpen ? vercelDetail(hooks.closeDetail) : vercelCard(hooks.openDetail);
  }
  if (!hasIntegrationData(task.id)) return idleCard(task, hooks.openSettings);

  switch (task.id) {
    case "integration_resend":
      return resendCard();
    case "integration_github":
      return githubCard();
    case "integration_gitlab":
      return gitlabCard(hooks.openGitlab);
    case "integration_jenkins":
      return jenkinsCard();
    case "integration_stripe":
      return stripeCard();
    case "integration_notion":
      return notionCard();
    case "integration_calcom":
      return calcomCard();
    default:
      return idleCard(task, hooks.openSettings);
  }
}

export { clear };
