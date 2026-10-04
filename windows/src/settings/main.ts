// Settings window — the place where anything that writes to disk is confirmed.
// Stage 2 covers the Claude Code hooks and the general preferences; API keys and
// integrations land here too in a later stage.

import "./settings.css";
import { Bridge, errorText, onEvent, type IdeStatus } from "../core/bridge";
import { applyLanguage, t } from "../core/i18n";
import { DEFAULT_SETTINGS, WORKSPACE_PILLS, type Settings } from "../core/state";
import { h, clear } from "../views/dom";

document.title = "Coucou";
const bootLabel = document.querySelector("#settings-root p");
if (bootLabel) bootLabel.textContent = "…";

const root = document.getElementById("settings-root")!;

let settings: Settings = { ...DEFAULT_SETTINGS };
let version = "";
let hasKey = false;
let secretsPresent: Record<string, boolean> = {};

function paint() {
  applyLanguage(settings.language);
  document.title = t("settings.windowTitle");
  clear(root);
  root.append(
    h("h1", {}, h("span", { text: "Coucou" }), h("span", { class: "version", text: version })),
    idesSection(),
    apiSection(hasKey),
    integrationsSection(secretsPresent),
    generalSection(),
    h("div", {
      class: "hint",
      text: t("settings.privacy"),
    }),
  );
}

// ── Boot ──────────────────────────────────────────────────────────────────────

async function main() {
  const boot = await Bridge.boot();
  if (boot) {
    settings = { ...settings, ...boot.settings };
    version = boot.version;
  }

  hasKey = (await Bridge.secretPresent("anthropic-api-key")) ?? false;

  const keys = [
    "stripe-api-key", "github-token", "vercel-token",
    "n8n-url", "n8n-api-key", "resend-api-key", "notion-api-key", "calcom-api-key",
  ];
  for (const k of keys) secretsPresent[k] = (await Bridge.secretPresent(k)) ?? false;

  paint();

  void onEvent<Settings>("settings-changed", (s) => {
    const previous = settings.language;
    settings = { ...settings, ...s };
    if (settings.language !== previous) paint();
  });
}

async function save() {
  await Bridge.saveSettings(settings);
}

// ── Reusable bits ─────────────────────────────────────────────────────────────

function toggle(on: boolean, onChange: (v: boolean) => void): HTMLElement {
  const el = h("button", { class: on ? "switch on" : "switch", "aria-pressed": on });
  el.addEventListener("click", () => {
    const next = !el.classList.contains("on");
    el.classList.toggle("on", next);
    onChange(next);
  });
  return el;
}

function statusDot(ok: boolean): HTMLElement {
  return h("i", { class: "dot", style: `background:${ok ? "#22c55e" : "#f4505e"}` });
}

function renderDiff(text: string): HTMLElement {
  const box = h("div", { class: "diff" });
  for (const line of text.split("\n")) {
    const cls = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    box.append(h("div", { class: cls, text: line }));
  }
  return box;
}

// ── IDEs ──────────────────────────────────────────────────────────────────────

const IDE_LABEL: Record<IdeStatus["id"], string> = {
  claude: "Claude Code",
  cursor: "Cursor",
};

function idesSection(): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:12px" });
  const section = h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("hooks.ides") })),
    body,
  );
  let rows: IdeStatus[] = [];
  const wanted = new Map<string, boolean>();

  const rebuild = async () => {
    rows = (await Bridge.idesStatus()) ?? [];
    for (const row of rows) {
      if (!wanted.has(row.id)) wanted.set(row.id, row.installed);
    }
    const head = section.querySelector("h2")!;
    clear(head);
    head.append(
      statusDot(rows.some((row) => row.installed)),
      h("span", { text: t("hooks.ides") }),
    );
    clear(body);
    draw();
  };

  function draw() {
    body.append(h("div", { class: "hint", text: t("hooks.idesHint") }));
    const ready = rows.every((row) => row.hookReady);
    for (const row of rows) {
      const box = h("input", { type: "checkbox" }) as HTMLInputElement;
      box.checked = wanted.get(row.id) ?? row.installed;
      box.addEventListener("change", () => wanted.set(row.id, box.checked));
      const pick = h("label", {
        style: "display:flex;align-items:center;gap:10px;min-width:148px;cursor:pointer;color:var(--ink)",
      }, box, h("span", { text: IDE_LABEL[row.id] ?? row.id }));
      body.append(h(
        "div",
        { class: "row" },
        pick,
        h("span", { class: "path", text: row.settingsPath }),
        statusDot(row.installed),
      ));
    }
    if (rows[0]) {
      body.append(h("div", { class: "row" },
        h("label", { text: t("hooks.relay") }),
        h("span", { class: "path", text: rows[0].hookPath }),
        statusDot(rows[0].hookReady),
      ));
    }
    if (!ready) {
      body.append(h("div", { class: "notice warn", text: t("hooks.missingExe") }));
    }
    const connect = h("button", {
      class: "primary",
      text: t("hooks.branch"),
      onclick: () => void showPreview(),
    });
    if (!ready) {
      connect.disabled = true;
      connect.title = t("hooks.relayMissingTitle");
    }
    body.append(h("div", { class: "row" }, connect));
  }

  async function showPreview() {
    let preview;
    try {
      preview = await Bridge.idesPreview(
        rows.map((row) => ({ id: row.id, install: wanted.get(row.id) ?? false })),
      );
    } catch (err) {
      clear(body);
      body.append(
        h("div", { class: "notice err", text: String(err).replace(/^Error:\s*/, "") }),
        h("div", { class: "row" }, h("button", {
          text: t("common.back"),
          onclick: () => { clear(body); draw(); },
        })),
      );
      return;
    }
    clear(body);
    if (preview.changes.length === 0) {
      body.append(
        h("div", { class: "hint", text: t("hooks.nothingToDo") }),
        h("div", { class: "row" }, h("button", {
          text: t("common.back"),
          onclick: () => { clear(body); draw(); },
        })),
      );
      return;
    }
    body.append(h("div", { class: "hint", text: t("hooks.previewIdes") }));
    for (const change of preview.changes) {
      body.append(
        h("div", {
          class: "hint",
          text: `${IDE_LABEL[change.id] ?? change.id} · ${change.settingsPath}`,
        }),
        renderDiff(change.diff),
        h("div", { class: "row" },
          h("span", { class: "path", text: t("hooks.backup", { path: change.backup }) }),
        ),
      );
    }
    const installs = preview.changes.some((change) => change.install);
    const confirm = h("button", {
      class: installs ? "primary" : "danger",
      text: installs ? t("hooks.backupWrite") : t("hooks.backupRemove"),
    });
    confirm.addEventListener("click", async () => {
      confirm.disabled = true;
      try {
        const backup = await Bridge.idesApply(preview.changes.map((change) => ({
          id: change.id,
          install: change.install,
          fingerprint: change.fingerprint,
        })));
        for (const change of preview.changes) wanted.set(change.id, change.install);
        clear(body);
        body.append(h("div", {
          class: "notice ok",
          text: backup.trim()
            ? t("hooks.doneIdes", { backup })
            : t("hooks.doneIdesNew"),
        }));
        window.setTimeout(() => void rebuild(), 2600);
      } catch (err) {
        confirm.disabled = false;
        body.append(h("div", { class: "notice err", text: t("hooks.writeFailed", { error: String(err) }) }));
      }
    });
    body.append(h("div", { class: "row" }, confirm, h("button", {
      text: t("common.cancel"),
      onclick: () => { clear(body); draw(); },
    })));
  }

  void rebuild();
  return section;
}

// ── Claude API section ────────────────────────────────────────────────────────

function modelLabel(id: string): string {
  const cut = Math.max(id.lastIndexOf("/"), id.lastIndexOf("\\"));
  return cut >= 0 && cut < id.length - 1 ? id.slice(cut + 1) : id;
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(t("chat.ollamaTimeout"))), ms);
    promise.then(resolve, reject).finally(() => clearTimeout(timer));
  });
}

const MODELS: [string, string][] = [
  ["claude-opus-5", "Claude Opus 5"],
  ["claude-sonnet-5", "Claude Sonnet 5"],
  ["claude-haiku-4-5", "Claude Haiku 4.5"],
];

function apiSection(hasKey: boolean): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:12px" });
  const section = h("section", {}, h("h2"), body);
  let keyPresent = hasKey;
  let paintGen = 0;

  function draw() {
    const gen = ++paintGen;
    const ollama = settings.chatProvider === "ollama";
    const head = section.querySelector("h2")!;
    clear(head);
    head.append(
      statusDot(ollama ? true : keyPresent),
      h("span", { text: ollama ? "Ollama" : "Claude" }),
    );
    if (ollama) {
      const dot = head.querySelector(".dot") as HTMLElement | null;
      if (dot) dot.style.background = "#facc15";
    }
    clear(body);

    const provider = h("select", {}) as HTMLSelectElement;
    provider.append(
      h("option", { value: "claude", text: "Claude" }),
      h("option", { value: "ollama", text: "Ollama" }),
    );
    provider.value = ollama ? "ollama" : "claude";
    provider.addEventListener("change", () => {
      const next = provider.value === "ollama" ? "ollama" : "claude";
      settings.chatProvider = next;
      if (next === "claude" && !MODELS.some(([id]) => id === settings.model)) {
        settings.model = "claude-opus-5";
      }
      if (next === "ollama" && settings.model.startsWith("claude-")) settings.model = "";
      void save();
      draw();
    });
    body.append(h("div", { class: "row" }, h("label", { text: t("chat.provider") }), provider));
    if (ollama) drawOllama();
    else drawClaude();
  }

  function drawClaude() {
    const dot = section.querySelector(".dot") as HTMLElement;
    const state = h("span", { class: "hint", text: keyPresent ? t("chat.keySavedWin") : t("chat.noKey") });
    const field = h("input", {
      type: "password",
      placeholder: keyPresent ? t("chat.stored") : "sk-ant-...",
      style: "flex:1 1 auto;min-width:0",
      autocomplete: "off",
      spellcheck: "false",
    }) as HTMLInputElement;
    const saveBtn = h("button", { class: "primary", text: t("chat.saveKey") });
    const clearBtn = h("button", { class: "danger", text: t("common.remove") });
    const feedback = h("div", {});
    clearBtn.style.display = keyPresent ? "" : "none";

    async function refresh() {
      keyPresent = (await Bridge.secretPresent("anthropic-api-key")) ?? false;
      dot.style.background = keyPresent ? "#22c55e" : "#f4505e";
      state.textContent = keyPresent ? t("chat.keySavedWin") : t("chat.noKey");
      field.placeholder = keyPresent ? t("chat.stored") : "sk-ant-...";
      clearBtn.style.display = keyPresent ? "" : "none";
    }

    saveBtn.addEventListener("click", async () => {
      const value = field.value.trim();
      if (!value) return;
      clear(feedback);
      try {
        await Bridge.secretSet("anthropic-api-key", value);
        field.value = "";
        feedback.append(h("div", { class: "notice ok", text: t("chat.savedDisk") }));
        await refresh();
      } catch (err) {
        feedback.append(h("div", { class: "notice err", text: t("chat.saveFailed", { error: String(err) }) }));
      }
    });
    clearBtn.addEventListener("click", async () => {
      clear(feedback);
      try {
        await Bridge.secretClear("anthropic-api-key");
        feedback.append(h("div", { class: "notice ok", text: t("chat.keyRemoved") }));
        await refresh();
      } catch (err) {
        feedback.append(h("div", { class: "notice err", text: t("chat.removeFailed", { error: String(err) }) }));
      }
    });

    const model = h("select", {}) as HTMLSelectElement;
    for (const [id, label] of MODELS) model.append(h("option", { value: id, text: label }));
    if (settings.model && !MODELS.some(([id]) => id === settings.model)) {
      model.append(h("option", { value: settings.model, text: settings.model }));
    }
    model.value = settings.model;
    model.addEventListener("change", () => {
      settings.model = model.value;
      void save();
    });

    body.append(
      state,
      h("div", { class: "row" }, h("label", { text: t("chat.key") }), field, saveBtn, clearBtn),
      h("div", { class: "row" }, h("label", { text: t("chat.model") }), model),
      feedback,
    );
  }

  function drawOllama() {
    const gen = paintGen;
    const dot = section.querySelector(".dot") as HTMLElement;
    const url = h("input", {
      type: "text",
      value: settings.ollamaUrl,
      placeholder: "http://127.0.0.1:11434",
      style: "flex:1 1 auto;min-width:0",
      spellcheck: "false",
    }) as HTMLInputElement;
    const connect = h("button", { class: "primary", text: t("chat.connect") });
    const model = h("select", {
      style: "flex:1 1 240px;min-width:0;max-width:100%",
    }) as HTMLSelectElement;
    const feedback = h("div", {});

    function fill(models: string[]): boolean {
      clear(model);
      const ids = [...new Set(models.filter(Boolean))];
      if (settings.model && !ids.includes(settings.model)) ids.unshift(settings.model);
      if (ids.length === 0) {
        model.title = "";
        model.append(h("option", { value: "", text: t("chat.pickModel") }));
        return false;
      }
      for (const id of ids) {
        model.append(h("option", { value: id, text: modelLabel(id), title: id }));
      }
      const wanted = ids.includes(settings.model) ? settings.model : ids[0];
      const match = [...model.options].find((option) => option.getAttribute("value") === wanted);
      if (match) match.selected = true;
      const chosen = match?.getAttribute("value") ?? "";
      model.title = chosen;
      if (!chosen || chosen === settings.model) return false;
      settings.model = chosen;
      return true;
    }

    model.addEventListener("change", () => {
      settings.model = model.selectedOptions[0]?.getAttribute("value") || model.value;
      model.title = settings.model;
      void save();
    });

    async function load() {
      connect.disabled = true;
      connect.textContent = t("chat.connecting");
      clear(feedback);
      const endpoint = url.value.trim() || settings.ollamaUrl;
      let picked = false;
      try {
        const models = await withTimeout(Bridge.ollamaModels(endpoint), 30_000);
        if (gen !== paintGen) return;
        picked = fill(models);
        dot.style.background = models.length ? "#22c55e" : "#f5a524";
        if (models.length === 0) {
          feedback.append(h("div", { class: "notice warn", text: t("status.noModels", { name: "Ollama" }) }));
        } else {
          feedback.append(h("div", { class: "notice ok", text: t("chat.connected") }));
        }
      } catch (err) {
        const message = errorText(err) || t("chat.ollamaDown");
        if (gen !== paintGen) return;
        dot.style.background = "#f4505e";
        fill(settings.model ? [settings.model] : []);
        feedback.append(h("div", { class: "notice err", text: message }));
      } finally {
        if (gen !== paintGen) return;
        if (picked) void save();
        connect.disabled = false;
        connect.textContent = t("chat.connect");
      }
    }

    connect.addEventListener("click", () => {
      settings.ollamaUrl = url.value.trim() || "http://127.0.0.1:11434";
      url.value = settings.ollamaUrl;
      void save().then(() => load());
    });

    if (settings.model) fill([settings.model]);
    else model.append(h("option", { value: "", text: t("chat.pickModel") }));
    body.append(
      h("div", { class: "hint", text: t("chat.localHint") }),
      h("div", { class: "row" }, h("label", { text: t("chat.server") }), url, connect),
      h("div", { class: "row" }, h("label", { text: t("chat.model") }), model),
      feedback,
    );
  }

  draw();
  return section;
}

// ── Integrations section ──────────────────────────────────────────────────────

interface IntegrationDef {
  id: string;
  name: string;
  color: string;
  /** Credential Manager keys, in the order they are shown. */
  fields: { key: string; label: string; placeholder: string; secret: boolean }[];
}

function integrationDefs(): IntegrationDef[] {
  return [
  { id: "integration_stripe", name: "Stripe", color: "#0570DE",
    fields: [{ key: "stripe-api-key", label: t("int.secret"), placeholder: "sk_live_…", secret: true }] },
  { id: "integration_github", name: "GitHub", color: "#F4505E",
    fields: [{ key: "github-token", label: t("int.token"), placeholder: "ghp_…", secret: true }] },
  { id: "integration_vercel", name: "Vercel", color: "#7C5CFF",
    fields: [{ key: "vercel-token", label: t("int.token"), placeholder: "…", secret: true }] },
  { id: "integration_n8n", name: "n8n", color: "#F29B38",
    fields: [
      { key: "n8n-url", label: t("int.instanceUrl"), placeholder: "https://n8n.example.com", secret: false },
      { key: "n8n-api-key", label: t("chat.key"), placeholder: "…", secret: true },
    ] },
  { id: "integration_resend", name: "Resend", color: "#22C55E",
    fields: [{ key: "resend-api-key", label: t("chat.key"), placeholder: "re_…", secret: true }] },
  { id: "integration_notion", name: "Notion", color: "#8C8C8C",
    fields: [{ key: "notion-api-key", label: t("int.integrationToken"), placeholder: "ntn_…", secret: true }] },
  { id: "integration_calcom", name: "Cal.com", color: "#C9956A",
    fields: [{ key: "calcom-api-key", label: t("chat.key"), placeholder: "cal_…", secret: true }] },
  ];
}

const MAX_ACTIVE = 4;

function integrationsSection(present: Record<string, boolean>): HTMLElement {
  const note = h("div", { class: "hint" });
  const list = h("div", { style: "display:flex;flex-direction:column;gap:14px" });

  function updateNote() {
    const used = settings.activeIntegrations.length;
    note.textContent = t("int.pick", { max: MAX_ACTIVE, used });
  }

  for (const def of integrationDefs()) {
    const active = settings.activeIntegrations.includes(def.id);
    const sw = h("button", { class: active ? "switch on" : "switch" });
    sw.addEventListener("click", () => {
      const on = settings.activeIntegrations.includes(def.id);
      if (on) {
        settings.activeIntegrations = settings.activeIntegrations.filter((x) => x !== def.id);
      } else {
        if (settings.activeIntegrations.length >= MAX_ACTIVE) return;
        settings.activeIntegrations = [...settings.activeIntegrations, def.id];
      }
      sw.classList.toggle("on", !on);
      updateNote();
      void save();
    });

    const rows = h("div", { style: "display:flex;flex-direction:column;gap:6px;flex:1 1 auto;min-width:0" });
    for (const field of def.fields) {
      const input = h("input", {
        type: field.secret ? "password" : "text",
        placeholder: present[field.key] ? t("chat.storedShort") : field.placeholder,
        autocomplete: "off",
        spellcheck: "false",
        style: "flex:1 1 auto;min-width:0",
      }) as HTMLInputElement;
      const saveBtn = h("button", { text: t("common.save") });
      const dotEl = statusDot(present[field.key] ?? false);
      saveBtn.addEventListener("click", async () => {
        const value = input.value.trim();
        try {
          await Bridge.secretSet(field.key, value);
          present[field.key] = value.length > 0;
          input.value = "";
          input.placeholder = value ? t("chat.storedShort") : field.placeholder;
          dotEl.style.background = value ? "#22c55e" : "#f4505e";
        } catch {
          dotEl.style.background = "#f5a524";
        }
      });
      rows.append(
        h("div", { class: "row" },
          h("label", { style: "min-width:104px", text: field.label }),
          input, saveBtn, dotEl,
        ),
      );
    }

    list.append(
      h("div", { style: "display:flex;gap:12px;align-items:flex-start" },
        h("div", { style: "display:flex;align-items:center;gap:8px;min-width:132px;padding-top:4px" },
          sw,
          h("i", { class: "dot", style: `background:${def.color}` }),
          h("span", { style: "font-size:12.5px", text: def.name }),
        ),
        rows,
      ),
    );
  }

  updateNote();
  return h("section", {}, h("h2", {}, h("span", { text: t("settings.integrations") })), note, list);
}

// ── General section ───────────────────────────────────────────────────────────

function generalSection(): HTMLElement {
  const volume = h("input", {
    type: "range", min: "0", max: "0.2", step: "0.005",
    value: String(settings.soundVolume),
  }) as HTMLInputElement;
  volume.addEventListener("input", () => {
    settings.soundVolume = Number(volume.value);
    void save();
  });

  const autoClose = h("input", {
    type: "number", min: "5", max: "120", step: "1",
    value: String(Math.round(settings.autoCloseInterval)),
    style: "width:72px",
  }) as HTMLInputElement;
  autoClose.addEventListener("change", () => {
    settings.autoCloseInterval = Math.max(5, Math.min(120, Number(autoClose.value) || 15));
    autoClose.value = String(settings.autoCloseInterval);
    void save();
  });

  const screen = h("select", {}) as HTMLSelectElement;
  screen.append(
    h("option", { value: "primary", text: t("settings.screenPrimary") }),
    h("option", { value: "cursor", text: t("settings.screenCursor") }),
  );
  screen.value = settings.screen;
  screen.addEventListener("change", () => {
    settings.screen = screen.value as Settings["screen"];
    void save();
  });

  const mainPill = h("select", {}) as HTMLSelectElement;
  for (const pill of WORKSPACE_PILLS) {
    mainPill.append(h("option", { value: pill.id, text: pill.name }));
  }
  mainPill.value = WORKSPACE_PILLS.some((pill) => pill.id === settings.mainPill)
    ? settings.mainPill
    : "integration_claude";
  mainPill.addEventListener("change", () => {
    const next = mainPill.value;
    if (!WORKSPACE_PILLS.some((pill) => pill.id === next) || next === settings.mainPill) return;
    settings.mainPill = next;
    void save();
  });

  const language = h("select", {}) as HTMLSelectElement;
  language.append(
    h("option", { value: "system", text: t("settings.languageSystem") }),
    h("option", { value: "fr", text: "Français" }),
    h("option", { value: "en", text: "English" }),
  );
  language.value = settings.language;
  language.addEventListener("change", () => {
    const value = language.value;
    const next = value === "en" || value === "fr" ? value : "system";
    if (next === settings.language) return;
    settings.language = next;
    void save();
    paint();
  });

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("settings.general") })),
    h("div", { class: "row" },
      h("label", { text: t("settings.language") }),
      language,
    ),
    h("div", { class: "row" },
      h("label", { text: t("pills.main") }),
      mainPill,
    ),
    h("div", { class: "row" },
      h("label", { text: t("settings.sound") }),
      toggle(settings.soundEnabled, (v) => { settings.soundEnabled = v; void save(); }),
      volume,
    ),
    h("div", { class: "row" },
      h("label", { text: t("settings.autoCloseLabel") }),
      autoClose,
      h("span", { class: "hint", text: t("settings.autoCloseHint") }),
    ),
    h("div", { class: "row" },
      h("label", { text: t("settings.screen") }),
      screen,
    ),
    h("div", { class: "row" },
      h("label", { text: t("settings.launch") }),
      toggle(settings.autostart, (v) => { settings.autostart = v; void save(); }),
    ),
  );
}

void main();
