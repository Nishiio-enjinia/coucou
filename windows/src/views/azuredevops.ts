// Full-width Azure DevOps browser. Pipelines lead; commits and open bugs
// sit on the same project. The overview card stays a short digest.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { t } from "../core/i18n";
import { State } from "../core/state";
import { Bridge, errorText, type AdoRow } from "../core/bridge";
import { timeAgo } from "./integrations";
import type { ViewHost } from "./views";

interface Project {
  id: string;
  name: string;
  description: string;
  updatedAt: string;
  url: string | null;
}

interface Repo {
  id: string;
  name: string;
  branch: string;
  url: string | null;
}

interface Commit {
  sha: string;
  shortId: string;
  title: string;
  message: string;
  author: string;
  email: string;
  createdAt: string;
  url: string | null;
}

interface Build {
  id: number;
  name: string;
  status: string;
  ref: string;
  sha: string;
  number: string;
  author: string;
  createdAt: string;
  url: string | null;
}

interface Stage {
  name: string;
  stage: string;
  status: string;
}

interface Bug {
  id: number;
  title: string;
  state: string;
  author: string;
  project: string;
  createdAt: string;
  url: string | null;
}

interface Page<T> {
  items: T[];
  continuation: string | null;
  done: boolean;
  loaded: boolean;
}

type Tab = "pipelines" | "commits" | "bugs";

type Place =
  | { at: "projects" }
  | { at: "project"; project: Project; tab: Tab }
  | { at: "commits"; project: Project; repo: Repo }
  | { at: "commit"; project: Project; repo: Repo; commit: Commit }
  | { at: "timeline"; project: Project; build: Build }
  | { at: "bug"; project: Project; bug: Bug };

const PIPELINE_STATUS = [
  "success", "failed", "running", "pending", "canceled", "canceling", "partial", "skipped", "other",
];
const BUG_STATES = ["New", "Active", "Resolved", "Closed", "Approved"];
const STEP_KINDS = ["Stage", "Phase", "Job", "Task"];

function str(row: AdoRow, key: string): string {
  const v = row[key];
  return typeof v === "string" ? v : "";
}

function num(row: AdoRow, key: string): number {
  const v = row[key];
  return typeof v === "number" ? v : 0;
}

function link(row: AdoRow): string | null {
  const url = str(row, "url");
  if (url.startsWith("https://") || url.startsWith("http://")) return url;
  return null;
}

function guid(id: string): boolean {
  return /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(id);
}

function asProject(row: AdoRow): Project | null {
  const id = str(row, "id");
  const name = str(row, "name");
  if (!guid(id) || !name) return null;
  return { id, name, description: str(row, "description"), updatedAt: str(row, "updatedAt"), url: link(row) };
}

function asRepo(row: AdoRow): Repo | null {
  const id = str(row, "id");
  const name = str(row, "name");
  if (!guid(id) || !name) return null;
  return { id, name, branch: str(row, "branch"), url: link(row) };
}

function asCommit(row: AdoRow): Commit | null {
  const sha = str(row, "sha");
  if (sha.length < 7) return null;
  return {
    sha,
    shortId: str(row, "shortId") || sha.slice(0, 8),
    title: str(row, "title"),
    message: str(row, "message"),
    author: str(row, "author"),
    email: str(row, "email"),
    createdAt: str(row, "createdAt"),
    url: link(row),
  };
}

function asBuild(row: AdoRow): Build | null {
  const id = num(row, "id");
  if (id <= 0) return null;
  return {
    id,
    name: str(row, "name"),
    status: str(row, "status") || "other",
    ref: str(row, "ref"),
    sha: str(row, "sha"),
    number: str(row, "number"),
    author: str(row, "author"),
    createdAt: str(row, "createdAt"),
    url: link(row),
  };
}

function asStage(row: AdoRow): Stage | null {
  const name = str(row, "name");
  if (!name) return null;
  return { name, stage: str(row, "stage"), status: str(row, "status") || "other" };
}

function asBug(row: AdoRow): Bug | null {
  const id = num(row, "id");
  const title = str(row, "title");
  if (id <= 0 || !title) return null;
  return {
    id,
    title,
    state: str(row, "state"),
    author: str(row, "author"),
    project: str(row, "project"),
    createdAt: str(row, "createdAt"),
    url: link(row),
  };
}

function statusColor(status: string): string {
  if (status === "success") return "#22C55E";
  if (status === "failed") return "#F4505E";
  if (status === "partial") return "#F5A524";
  if (status === "running" || status === "pending" || status === "canceling") return "#3B9EFF";
  return "#6B7079";
}

function statusLabel(status: string): string {
  if (PIPELINE_STATUS.includes(status)) return t(`ado.status.${status}`);
  return status === "other" ? t("ado.status.other") : status;
}

function bugColor(state: string): string {
  if (state === "Resolved" || state === "Closed") return "#22C55E";
  if (state === "New" || state === "Active" || state === "Approved") return "#F4505E";
  return "#F5A524";
}

function bugLabel(state: string): string {
  if (BUG_STATES.includes(state)) return t(`ado.bug.${state}`);
  return state;
}

function stepLabel(kind: string): string {
  if (STEP_KINDS.includes(kind)) return t(`ado.step.${kind}`);
  return kind;
}

function commitBody(commit: Commit): string {
  const msg = commit.message.trim();
  const title = commit.title.trim();
  if (!msg || msg === title) return "";
  if (msg.startsWith(`${title}\n`)) return msg.slice(title.length).trim();
  return msg;
}

function openExternal(url: string | null) {
  if (url) void Bridge.openUrl(url);
}

function freshPage<T>(): Page<T> {
  return { items: [], continuation: null, done: false, loaded: false };
}

function instanceUrl(): string | null {
  const url = State.integrations.integration_azuredevops?.data?.webUrl;
  if (typeof url === "string" && (url.startsWith("https://") || url.startsWith("http://"))) return url;
  return null;
}

export function buildAzureDevops(onLeave: () => void): ViewHost {
  const body = h("div", { class: "gl-body" });
  const el = h("div", { class: "view azuredevops" }, h("div", { class: "gl-shell" }, body));

  let place: Place = { at: "projects" };
  let started = false;
  let busy = false;
  let error = "";
  const projects: Page<Project> = freshPage();
  const repos = new Map<string, Page<Repo>>();
  const commits = new Map<string, Page<Commit>>();
  const pipelines = new Map<string, Page<Build>>();
  const bugs = new Map<string, Page<Bug>>();
  const stages = new Map<number, Stage[]>();

  function render(keepScroll = false) {
    const list = body.querySelector(".gl-list");
    const top = keepScroll && list instanceof HTMLElement ? list.scrollTop : 0;
    clear(body);
    if (place.at === "projects") renderProjects();
    else if (place.at === "project") renderProject(place.project, place.tab);
    else if (place.at === "commits") renderCommits(place.project, place.repo);
    else if (place.at === "commit") renderCommit(place.project, place.commit);
    else if (place.at === "timeline") renderTimeline(place.project, place.build);
    else renderBug(place.project, place.bug);
    const next = body.querySelector(".gl-list");
    if (keepScroll && next instanceof HTMLElement) next.scrollTop = top;
  }

  function back() {
    if (place.at === "projects") {
      onLeave();
      return;
    }
    if (place.at === "commit") {
      const many = (repos.get(place.project.id)?.items.length ?? 0) > 1;
      place = many
        ? { at: "commits", project: place.project, repo: place.repo }
        : { at: "project", project: place.project, tab: "commits" };
    } else if (place.at === "commits") {
      place = { at: "project", project: place.project, tab: "commits" };
    } else if (place.at === "timeline") {
      place = { at: "project", project: place.project, tab: "pipelines" };
    } else if (place.at === "bug") {
      place = { at: "project", project: place.project, tab: "bugs" };
    } else {
      place = { at: "projects" };
    }
    error = "";
    render();
  }

  function crumb(title: string, extra?: Node | null): HTMLElement {
    return h(
      "div",
      { class: "gl-head" },
      h("button", { class: "gl-back", title: t("ado.back"), onclick: back }, svg(ICONS.chevronLeft, 11, { stroke: 2.4 })),
      h("div", { class: "gl-crumb" }, h("b", { text: "Azure DevOps" }), h("span", { title, text: title })),
      extra ?? null,
    );
  }

  function notice(text: string): HTMLElement {
    return h("div", { class: "gl-empty", text });
  }

  function moreButton(load: () => void): HTMLElement {
    return h("button", { class: "gl-more", text: busy ? t("ado.loading") : t("ado.more"), onclick: load });
  }

  function textButton(label: string, url: string | null): HTMLElement | null {
    if (!url) return null;
    return h("button", { class: "gl-text", text: label, onclick: () => openExternal(url) });
  }

  function renderProjects() {
    const actions = h("div", { class: "gl-actions" });
    const site = instanceUrl();
    if (site) actions.append(h("button", { class: "gl-text", text: t("ado.open"), onclick: () => openExternal(site) }));
    actions.append(h("button", {
      class: "gl-text",
      text: t("int.refresh"),
      onclick: () => {
        projects.items = [];
        projects.continuation = null;
        projects.done = false;
        projects.loaded = false;
        void loadProjects(false);
      },
    }));
    body.append(crumb(t("ado.projects"), actions));
    const list = h("div", { class: "gl-list" });
    if (error && projects.items.length === 0) list.append(notice(error));
    else if (projects.items.length === 0) list.append(notice(busy ? t("ado.loading") : t("ado.noProjects")));
    for (const project of projects.items) {
      list.append(h(
        "button",
        { class: "gl-row", onclick: () => openProject(project) },
        h("span", { class: "gl-path", title: project.name, text: project.name }),
        h("span", { class: "gl-meta", title: project.description, text: project.description }),
        h("span", { class: "gl-ago", text: timeAgo(project.updatedAt) }),
      ));
    }
    if (!projects.done && projects.items.length > 0) list.append(moreButton(() => void loadProjects(true)));
    if (error && projects.items.length > 0) list.append(notice(error));
    body.append(list);
  }

  function renderProject(project: Project, tab: Tab) {
    const tabs = h("div", { class: "gl-tabs" });
    for (const name of ["pipelines", "commits", "bugs"] as const) {
      const label = name === "pipelines" ? t("ado.pipelines") : name === "commits" ? t("ado.commits") : t("ado.bugs");
      tabs.append(h("button", {
        class: name === tab ? "gl-tab on" : "gl-tab",
        text: label,
        onclick: () => selectTab(project, name),
      }));
    }
    const actions = h("div", { class: "gl-actions" }, tabs, textButton(t("ado.open"), project.url));
    body.append(crumb(project.name, actions));
    const list = h("div", { class: "gl-list" });
    if (tab === "pipelines") fillPipelines(list, project);
    else if (tab === "bugs") fillBugs(list, project);
    else fillCommitTab(list, project);
    if (error && list.childElementCount > 1) list.append(notice(error));
    body.append(list);
  }

  function fillPipelines(list: HTMLElement, project: Project) {
    const page = pipelines.get(project.id);
    const items = page?.items ?? [];
    if (error && items.length === 0) list.append(notice(error));
    else if (items.length === 0) list.append(notice(busy || !page?.loaded ? t("ado.loading") : t("ado.noPipelines")));
    for (const build of items) list.append(pipelineRow(project, build));
    if (page && !page.done && items.length > 0) list.append(moreButton(() => void loadPipelines(project, true)));
  }

  function fillBugs(list: HTMLElement, project: Project) {
    const page = bugs.get(project.id);
    const items = page?.items ?? [];
    if (error && items.length === 0) list.append(notice(error));
    else if (items.length === 0) list.append(notice(busy || !page?.loaded ? t("ado.loading") : t("ado.noBugs")));
    for (const bug of items) list.append(bugRow(project, bug));
  }

  function fillCommitTab(list: HTMLElement, project: Project) {
    const page = repos.get(project.id);
    const items = page?.items ?? [];
    if (!page?.loaded) {
      list.append(notice(error || t("ado.loading")));
      return;
    }
    if (items.length === 1) {
      fillCommits(list, project, items[0]);
      return;
    }
    if (error && items.length === 0) list.append(notice(error));
    else if (items.length === 0) list.append(notice(t("ado.noRepos")));
    for (const repo of items) {
      list.append(h(
        "button",
        {
          class: "gl-row",
          onclick: () => {
            place = { at: "commits", project, repo };
            error = "";
            render();
            void loadCommits(project, repo, false);
          },
        },
        h("span", { class: "gl-path", title: repo.name, text: repo.name }),
        h("span", { class: "gl-meta", text: repo.branch }),
      ));
    }
  }

  function fillCommits(list: HTMLElement, project: Project, repo: Repo) {
    const page = commits.get(repo.id);
    const items = page?.items ?? [];
    if (error && items.length === 0) list.append(notice(error));
    else if (items.length === 0) list.append(notice(busy || !page?.loaded ? t("ado.loading") : t("ado.noCommits")));
    for (const commit of items) list.append(commitRow(project, repo, commit));
    if (page && !page.done && items.length > 0) list.append(moreButton(() => void loadCommits(project, repo, true)));
  }

  function renderCommits(project: Project, repo: Repo) {
    body.append(crumb(`${project.name} / ${repo.name}`, textButton(t("ado.open"), repo.url)));
    const list = h("div", { class: "gl-list" });
    fillCommits(list, project, repo);
    body.append(list);
  }

  function commitRow(project: Project, repo: Repo, commit: Commit): HTMLElement {
    return h(
      "button",
      { class: "gl-row commit", onclick: () => { place = { at: "commit", project, repo, commit }; error = ""; render(); } },
      h("span", { class: "gl-sha", text: commit.shortId }),
      h("span", { class: "gl-subject", title: commit.title, text: commit.title || commit.shortId }),
      h("span", { class: "gl-author", title: commit.author, text: commit.author }),
      h("span", { class: "gl-ago", text: timeAgo(commit.createdAt) }),
    );
  }

  function renderCommit(project: Project, commit: Commit) {
    body.append(crumb(project.name, textButton(t("ado.open"), commit.url)));
    const bodyText = commitBody(commit);
    const who = [commit.author, commit.email].filter(Boolean).join(" · ");
    body.append(h(
      "div",
      { class: "gl-list detail" },
      h("div", { class: "gl-title", text: commit.title || commit.shortId }),
      bodyText ? h("pre", { class: "gl-message", text: bodyText }) : null,
      h("div", { class: "gl-who", text: who }),
      h("div", { class: "gl-sha full", text: commit.sha }),
      h("div", { class: "gl-ago", text: timeAgo(commit.createdAt) }),
    ));
  }

  function pipelineRow(project: Project, build: Build): HTMLElement {
    const subject = build.number ? `${build.name} · ${build.number}` : build.name;
    return h(
      "button",
      {
        class: "gl-row",
        onclick: () => {
          place = { at: "timeline", project, build };
          error = "";
          render();
          void loadTimeline(project, build);
        },
      },
      dot(statusColor(build.status), 7),
      h("span", { class: "gl-status", style: `color:${statusColor(build.status)}`, text: statusLabel(build.status) }),
      h("span", { class: "gl-subject", title: subject, text: subject || "—" }),
      h("span", { class: "gl-meta", title: build.ref, text: build.ref }),
      h("span", { class: "gl-sha", text: build.sha }),
      h("span", { class: "gl-ago", text: timeAgo(build.createdAt) }),
    );
  }

  function renderTimeline(project: Project, build: Build) {
    const label = build.ref ? `${build.name} · ${build.ref}` : build.name;
    body.append(crumb(`${project.name} · ${label}`, textButton(t("ado.open"), build.url)));
    const list = h("div", { class: "gl-list" });
    const items = stages.get(build.id);
    if (error && !items) list.append(notice(error));
    else if (!items) list.append(notice(t("ado.loading")));
    else if (items.length === 0) list.append(notice(t("ado.noStages")));
    else {
      for (const stage of items) {
        list.append(h(
          "div",
          { class: "gl-row" },
          dot(statusColor(stage.status), 7),
          h("span", { class: "gl-status", style: `color:${statusColor(stage.status)}`, text: statusLabel(stage.status) }),
          h("span", { class: "gl-subject", title: stage.name, text: stage.name }),
          h("span", { class: "gl-meta", text: stepLabel(stage.stage) }),
        ));
      }
    }
    body.append(list);
  }

  function bugRow(project: Project, bug: Bug): HTMLElement {
    return h(
      "button",
      { class: "gl-row", onclick: () => { place = { at: "bug", project, bug }; error = ""; render(); } },
      dot(bugColor(bug.state), 7),
      h("span", { class: "gl-status", style: `color:${bugColor(bug.state)}`, text: bugLabel(bug.state) }),
      h("span", { class: "gl-subject", title: bug.title, text: `#${bug.id} ${bug.title}` }),
      h("span", { class: "gl-author", title: bug.author, text: bug.author }),
      h("span", { class: "gl-ago", text: timeAgo(bug.createdAt) }),
    );
  }

  function renderBug(project: Project, bug: Bug) {
    body.append(crumb(`${project.name} #${bug.id}`, textButton(t("ado.open"), bug.url)));
    body.append(h(
      "div",
      { class: "gl-list detail" },
      h("div", { class: "gl-title", text: bug.title }),
      h("div", { class: "gl-status", style: `color:${bugColor(bug.state)}`, text: bugLabel(bug.state) }),
      bug.author ? h("div", { class: "gl-who", text: bug.author }) : null,
      h("div", { class: "gl-ago", text: timeAgo(bug.createdAt) }),
    ));
  }

  function selectTab(project: Project, tab: Tab) {
    place = { at: "project", project, tab };
    error = "";
    render();
    if (tab === "pipelines") void loadPipelines(project, false);
    else if (tab === "bugs") void loadBugs(project);
    else void loadRepos(project);
  }

  async function fetchPage(req: {
    kind: "projects" | "repos" | "commits" | "pipelines" | "timeline" | "bugs";
    projectId?: string;
    repositoryId?: string;
    buildId?: number;
    continuation?: string;
  }) {
    busy = true;
    error = "";
    render(true);
    try {
      const page = await Bridge.azuredevopsBrowse(req);
      busy = false;
      const items = Array.isArray(page?.items) ? page.items : [];
      const continuation = typeof page?.continuation === "string" && page.continuation ? page.continuation : null;
      return { items, continuation };
    } catch (err) {
      busy = false;
      error = errorText(err);
      render(true);
      return null;
    }
  }

  async function loadProjects(more: boolean) {
    if (projects.done) return;
    if (more && busy) return;
    if (!more && projects.loaded) return;
    const batch = await fetchPage({
      kind: "projects",
      continuation: more ? projects.continuation ?? undefined : undefined,
    });
    if (!batch) return;
    const rows = batch.items.map(asProject).filter((row): row is Project => row != null);
    if (!more) projects.items = [];
    projects.items.push(...rows);
    projects.continuation = batch.continuation;
    projects.done = !batch.continuation;
    projects.loaded = true;
    render(more);
  }

  async function loadRepos(project: Project) {
    let page = repos.get(project.id);
    if (!page) {
      page = freshPage();
      repos.set(project.id, page);
    }
    if (!page.loaded) {
      const batch = await fetchPage({ kind: "repos", projectId: project.id });
      if (!batch) return;
      page.items = batch.items.map(asRepo).filter((row): row is Repo => row != null);
      page.loaded = true;
      page.done = true;
      render();
    }
    const only = page.items.length === 1 ? page.items[0] : undefined;
    if (only && place.at === "project" && place.project.id === project.id && place.tab === "commits") {
      void loadCommits(project, only, false);
    }
  }

  async function loadCommits(project: Project, repo: Repo, more: boolean) {
    let page = commits.get(repo.id);
    if (!page) {
      page = freshPage();
      commits.set(repo.id, page);
    }
    if (page.done) return;
    if (!more && page.loaded) return;
    if (more && busy) return;
    const batch = await fetchPage({
      kind: "commits",
      projectId: project.id,
      repositoryId: repo.id,
      continuation: more ? page.continuation ?? undefined : undefined,
    });
    if (!batch) return;
    const rows = batch.items.map(asCommit).filter((row): row is Commit => row != null);
    if (!more) page.items = [];
    page.items.push(...rows);
    page.continuation = batch.continuation;
    page.done = !batch.continuation;
    page.loaded = true;
    render(more);
  }

  async function loadPipelines(project: Project, more: boolean) {
    let page = pipelines.get(project.id);
    if (!page) {
      page = freshPage();
      pipelines.set(project.id, page);
    }
    if (page.done) return;
    if (!more && page.loaded) return;
    if (more && busy) return;
    const batch = await fetchPage({
      kind: "pipelines",
      projectId: project.id,
      continuation: more ? page.continuation ?? undefined : undefined,
    });
    if (!batch) return;
    const rows = batch.items.map(asBuild).filter((row): row is Build => row != null);
    if (!more) page.items = [];
    page.items.push(...rows);
    page.continuation = batch.continuation;
    page.done = !batch.continuation;
    page.loaded = true;
    render(more);
  }

  async function loadBugs(project: Project) {
    let page = bugs.get(project.id);
    if (!page) {
      page = freshPage();
      bugs.set(project.id, page);
    }
    if (page.loaded) return;
    const batch = await fetchPage({ kind: "bugs", projectId: project.id });
    if (!batch) return;
    page.items = batch.items.map(asBug).filter((row): row is Bug => row != null);
    page.loaded = true;
    page.done = true;
    render();
  }

  async function loadTimeline(project: Project, build: Build) {
    if (stages.has(build.id)) return;
    const batch = await fetchPage({ kind: "timeline", projectId: project.id, buildId: build.id });
    if (!batch) return;
    stages.set(build.id, batch.items.map(asStage).filter((row): row is Stage => row != null));
    render();
  }

  function openProject(project: Project) {
    place = { at: "project", project, tab: "pipelines" };
    error = "";
    render();
    void loadPipelines(project, false);
  }

  return {
    el,
    sync() {
      if (started) return;
      started = true;
      void loadProjects(false);
    },
    escape() {
      if (place.at === "projects") return false;
      back();
      return true;
    },
  };
}
