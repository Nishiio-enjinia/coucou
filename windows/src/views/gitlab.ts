// Full-width GitLab browser. The overview card stays as it is; this view
// takes the whole panel so a path, a commit message and a pipeline fit.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { t } from "../core/i18n";
import { State } from "../core/state";
import { Bridge, errorText, type GitlabRow } from "../core/bridge";
import { timeAgo } from "./integrations";
import type { ViewHost } from "./views";

const PAGE = 40;

interface Project {
  id: number;
  name: string;
  path: string;
  branch: string;
  activityAt: string;
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

interface Pipeline {
  id: number;
  status: string;
  ref: string;
  sha: string;
  createdAt: string;
  url: string | null;
}

interface Job {
  id: number;
  name: string;
  stage: string;
  status: string;
  url: string | null;
}

interface Page<T> {
  items: T[];
  page: number;
  done: boolean;
}

type Tab = "commits" | "pipelines";

type Place =
  | { at: "projects" }
  | { at: "repo"; project: Project; tab: Tab }
  | { at: "commit"; project: Project; commit: Commit }
  | { at: "jobs"; project: Project; pipeline: Pipeline };

function str(row: GitlabRow, key: string): string {
  const v = row[key];
  return typeof v === "string" ? v : "";
}

function num(row: GitlabRow, key: string): number {
  const v = row[key];
  return typeof v === "number" ? v : 0;
}

function link(row: GitlabRow): string | null {
  const url = str(row, "url");
  if (url.startsWith("https://") || url.startsWith("http://")) return url;
  return null;
}

function asProject(row: GitlabRow): Project | null {
  const id = num(row, "id");
  const path = str(row, "path");
  if (id <= 0 || !path) return null;
  return {
    id,
    name: str(row, "name") || path,
    path,
    branch: str(row, "branch"),
    activityAt: str(row, "activityAt"),
    url: link(row),
  };
}

function asCommit(row: GitlabRow): Commit | null {
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

function asPipeline(row: GitlabRow): Pipeline | null {
  const id = num(row, "id");
  if (id <= 0) return null;
  return {
    id,
    status: str(row, "status") || "other",
    ref: str(row, "ref"),
    sha: str(row, "sha"),
    createdAt: str(row, "createdAt"),
    url: link(row),
  };
}

function asJob(row: GitlabRow): Job | null {
  const id = num(row, "id");
  const name = str(row, "name");
  if (id <= 0 || !name) return null;
  return { id, name, stage: str(row, "stage"), status: str(row, "status") || "other", url: link(row) };
}

function statusColor(status: string): string {
  if (status === "success") return "#22C55E";
  if (status === "failed") return "#F4505E";
  if (status === "running" || status === "pending" || status === "preparing" || status === "created" || status === "scheduled") {
    return "#3B9EFF";
  }
  return "#6B7079";
}

function statusLabel(status: string): string {
  const known = [
    "success", "failed", "running", "pending", "canceled", "canceling",
    "skipped", "manual", "created", "preparing", "scheduled",
  ];
  if (known.includes(status)) return t(`gitlab.status.${status}`);
  return status === "other" ? t("gitlab.status.other") : status;
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

function instanceUrl(): string | null {
  const url = State.integrations.integration_gitlab?.data?.webUrl;
  if (typeof url === "string" && (url.startsWith("https://") || url.startsWith("http://"))) return url;
  return null;
}

export function buildGitlab(onLeave: () => void): ViewHost {
  const body = h("div", { class: "gl-body" });
  const el = h("div", { class: "view gitlab" }, h("div", { class: "gl-shell" }, body));

  let place: Place = { at: "projects" };
  let started = false;
  let busy = false;
  let error = "";
  const projects: Page<Project> = { items: [], page: 0, done: false };
  const commits = new Map<number, Page<Commit>>();
  const pipelines = new Map<number, Page<Pipeline>>();
  const jobs = new Map<number, Job[]>();

  function render(keepScroll = false) {
    const list = body.querySelector(".gl-list");
    const top = keepScroll && list instanceof HTMLElement ? list.scrollTop : 0;
    clear(body);
    if (place.at === "projects") renderProjects();
    else if (place.at === "repo") renderRepo(place.project, place.tab);
    else if (place.at === "commit") renderCommit(place.project, place.commit);
    else renderJobs(place.project, place.pipeline);
    const next = body.querySelector(".gl-list");
    if (keepScroll && next instanceof HTMLElement) next.scrollTop = top;
  }

  function back() {
    if (place.at === "projects") {
      onLeave();
      return;
    }
    if (place.at === "commit") {
      place = { at: "repo", project: place.project, tab: "commits" };
    } else if (place.at === "jobs") {
      place = { at: "repo", project: place.project, tab: "pipelines" };
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
      h("button", { class: "gl-back", title: t("gitlab.back"), onclick: back }, svg(ICONS.chevronLeft, 11, { stroke: 2.4 })),
      h("div", { class: "gl-crumb" }, h("b", { text: "GitLab" }), h("span", { title, text: title })),
      extra ?? null,
    );
  }

  function notice(text: string): HTMLElement {
    return h("div", { class: "gl-empty", text });
  }

  function moreButton(load: () => void): HTMLElement {
    return h("button", { class: "gl-more", text: busy ? t("gitlab.loading") : t("gitlab.more"), onclick: load });
  }

  function renderProjects() {
    const actions = h("div", { class: "gl-actions" });
    const site = instanceUrl();
    if (site) {
      actions.append(h("button", { class: "gl-text", text: t("gitlab.open"), onclick: () => openExternal(site) }));
    }
    actions.append(h("button", {
      class: "gl-text",
      text: t("int.refresh"),
      onclick: () => {
        projects.items = [];
        projects.page = 0;
        projects.done = false;
        void loadProjects();
      },
    }));
    body.append(crumb(t("gitlab.repos"), actions));
    const list = h("div", { class: "gl-list" });
    if (error && projects.items.length === 0) list.append(notice(error));
    else if (projects.items.length === 0) list.append(notice(busy ? t("gitlab.loading") : t("gitlab.noRepos")));
    for (const project of projects.items) {
      const row = h(
        "button",
        { class: "gl-row", onclick: () => openProject(project) },
        h("span", { class: "gl-path", title: project.path, text: project.path }),
        h("span", { class: "gl-meta", text: project.branch }),
        h("span", { class: "gl-ago", text: timeAgo(project.activityAt) }),
      );
      list.append(row);
    }
    if (!projects.done && projects.items.length > 0) list.append(moreButton(() => void loadProjects()));
    if (error && projects.items.length > 0) list.append(notice(error));
    body.append(list);
  }

  function renderRepo(project: Project, tab: Tab) {
    const tabs = h("div", { class: "gl-tabs" });
    for (const name of ["commits", "pipelines"] as const) {
      tabs.append(h("button", {
        class: name === tab ? "gl-tab on" : "gl-tab",
        text: name === "commits" ? t("gitlab.commits") : t("gitlab.cicd"),
        onclick: () => {
          place = { at: "repo", project, tab: name };
          error = "";
          render();
          if (name === "commits") void loadCommits(project, false);
          else void loadPipelines(project, false);
        },
      }));
    }
    const open = project.url
      ? h("button", { class: "gl-text", text: t("gitlab.open"), onclick: () => openExternal(project.url) })
      : null;
    const branch = project.branch ? h("span", { class: "gl-meta", title: project.branch, text: project.branch }) : null;
    body.append(crumb(project.path, h("div", { class: "gl-actions" }, branch, tabs, open)));
    const list = h("div", { class: "gl-list" });
    if (tab === "commits") {
      const page = commits.get(project.id);
      const items = page?.items ?? [];
      if (error && items.length === 0) list.append(notice(error));
      else if (items.length === 0) list.append(notice(busy ? t("gitlab.loading") : t("gitlab.noCommits")));
      for (const commit of items) list.append(commitRow(project, commit));
      if (page && !page.done && items.length > 0) {
        list.append(moreButton(() => void loadCommits(project, true)));
      }
    } else {
      const page = pipelines.get(project.id);
      const items = page?.items ?? [];
      if (error && items.length === 0) list.append(notice(error));
      else if (items.length === 0) list.append(notice(busy ? t("gitlab.loading") : t("gitlab.noPipelines")));
      for (const pipeline of items) list.append(pipelineRow(project, pipeline));
      if (page && !page.done && items.length > 0) {
        list.append(moreButton(() => void loadPipelines(project, true)));
      }
    }
    if (error && list.childElementCount > 1) list.append(notice(error));
    body.append(list);
  }

  function commitRow(project: Project, commit: Commit): HTMLElement {
    return h(
      "button",
      { class: "gl-row commit", onclick: () => { place = { at: "commit", project, commit }; error = ""; render(); } },
      h("span", { class: "gl-sha", text: commit.shortId }),
      h("span", { class: "gl-subject", title: commit.title, text: commit.title || commit.shortId }),
      h("span", { class: "gl-author", title: commit.author, text: commit.author }),
      h("span", { class: "gl-ago", text: timeAgo(commit.createdAt) }),
    );
  }

  function renderCommit(project: Project, commit: Commit) {
    const open = commit.url
      ? h("button", { class: "gl-text", text: t("gitlab.open"), onclick: () => openExternal(commit.url) })
      : null;
    body.append(crumb(project.path, open));
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

  function pipelineRow(project: Project, pipeline: Pipeline): HTMLElement {
    return h(
      "button",
      {
        class: "gl-row",
        onclick: () => {
          place = { at: "jobs", project, pipeline };
          error = "";
          render();
          void loadJobs(project, pipeline);
        },
      },
      dot(statusColor(pipeline.status), 7),
      h("span", { class: "gl-status", style: `color:${statusColor(pipeline.status)}`, text: statusLabel(pipeline.status) }),
      h("span", { class: "gl-subject", title: pipeline.ref, text: pipeline.ref || "—" }),
      h("span", { class: "gl-sha", text: pipeline.sha }),
      h("span", { class: "gl-ago", text: timeAgo(pipeline.createdAt) }),
    );
  }

  function renderJobs(project: Project, pipeline: Pipeline) {
    const open = pipeline.url
      ? h("button", { class: "gl-text", text: t("gitlab.open"), onclick: () => openExternal(pipeline.url) })
      : null;
    const label = pipeline.ref ? `${pipeline.ref} · ${statusLabel(pipeline.status)}` : statusLabel(pipeline.status);
    body.append(crumb(`${project.path} · ${label}`, open));
    const list = h("div", { class: "gl-list" });
    const items = jobs.get(pipeline.id);
    if (error && !items) list.append(notice(error));
    else if (!items) list.append(notice(t("gitlab.loading")));
    else if (items.length === 0) list.append(notice(t("gitlab.noJobs")));
    else {
      for (const job of items) {
        const row = h(
          "button",
          { class: "gl-row", onclick: () => openExternal(job.url) },
          dot(statusColor(job.status), 7),
          h("span", { class: "gl-status", style: `color:${statusColor(job.status)}`, text: statusLabel(job.status) }),
          h("span", { class: "gl-subject", title: job.name, text: job.name }),
          h("span", { class: "gl-meta", text: job.stage }),
        );
        if (!job.url) row.disabled = true;
        list.append(row);
      }
    }
    body.append(list);
  }

  async function fetchPage(kind: "projects" | "commits" | "pipelines" | "jobs", projectId?: number, pipelineId?: number, page?: number) {
    busy = true;
    error = "";
    render(true);
    try {
      const rows = await Bridge.gitlabBrowse({ kind, projectId, pipelineId, page });
      busy = false;
      return rows;
    } catch (err) {
      busy = false;
      error = errorText(err);
      render(true);
      return null;
    }
  }

  async function loadProjects() {
    if (projects.done) return;
    if (projects.page > 0 && busy) return;
    const next = projects.page + 1;
    const rows = await fetchPage("projects", undefined, undefined, next);
    if (!rows) return;
    const batch = rows.map(asProject).filter((p): p is Project => p != null);
    projects.items.push(...batch);
    projects.page = next;
    projects.done = batch.length < PAGE;
    render(next > 1);
  }

  async function loadCommits(project: Project, more: boolean) {
    let page = commits.get(project.id);
    if (!page) {
      page = { items: [], page: 0, done: false };
      commits.set(project.id, page);
    }
    if (page.done) return;
    if (!more && page.items.length > 0) return;
    if (more && busy) return;
    const next = page.page + 1;
    const rows = await fetchPage("commits", project.id, undefined, next);
    if (!rows) return;
    const batch = rows.map(asCommit).filter((c): c is Commit => c != null);
    page.items.push(...batch);
    page.page = next;
    page.done = batch.length < PAGE;
    render(more);
  }

  async function loadPipelines(project: Project, more: boolean) {
    let page = pipelines.get(project.id);
    if (!page) {
      page = { items: [], page: 0, done: false };
      pipelines.set(project.id, page);
    }
    if (page.done) return;
    if (!more && page.items.length > 0) return;
    if (more && busy) return;
    const next = page.page + 1;
    const rows = await fetchPage("pipelines", project.id, undefined, next);
    if (!rows) return;
    const batch = rows.map(asPipeline).filter((p): p is Pipeline => p != null);
    page.items.push(...batch);
    page.page = next;
    page.done = batch.length < PAGE;
    render(more);
  }

  async function loadJobs(project: Project, pipeline: Pipeline) {
    if (jobs.has(pipeline.id) || busy) return;
    const rows = await fetchPage("jobs", project.id, pipeline.id);
    if (!rows) return;
    jobs.set(pipeline.id, rows.map(asJob).filter((j): j is Job => j != null));
    render();
  }

  function openProject(project: Project) {
    place = { at: "repo", project, tab: "commits" };
    error = "";
    render();
    void loadCommits(project, false);
  }

  return {
    el,
    sync() {
      if (started) return;
      started = true;
      void loadProjects();
    },
    escape() {
      if (place.at === "projects") return false;
      back();
      return true;
    },
  };
}
