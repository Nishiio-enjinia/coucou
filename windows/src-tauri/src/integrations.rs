// Integration pollers — the Rust side of StripePoller / GithubPoller /
// VercelPoller / N8nPoller / ResendPoller / NotionPoller / CalcomPoller.
//
// Same endpoints, same first-run delays and intervals as the Swift pollers. Each
// one emits an `integration` event; the island owns the badge, the sound and the
// 60 s auto-clear, exactly as the Swift handlers do.
//
// Nothing is polled until its key exists in the Credential Manager, and no
// request goes anywhere the user has not configured.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::island::WINDOW_LABEL;
use crate::log;
use crate::secrets;

const TIMEOUT: Duration = Duration::from_secs(10);

/// What the island receives. `event` is only set when something actually changed,
/// which is what drives the pill badge and the sound.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationUpdate {
    pub id: &'static str,
    pub data: Value,
    pub error: Option<String>,
    pub event: Option<IntegrationEvent>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationEvent {
    pub success: bool,
    pub label: String,
    pub detail: Option<String>,
    /// "working", "finished" or "error". Absent means finished when `success`, error otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'static str>,
}

fn emit(app: &AppHandle, update: IntegrationUpdate) {
    let _ = app.emit_to(WINDOW_LABEL, "integration", update);
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_default()
}

/// Set from the tray's Pause item. While it is on, nothing reaches the network:
/// pausing Coucou has to mean pausing Coucou, not just hiding the island.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn set_paused(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

/// Spawns every poller with the macOS delays and intervals.
pub fn start(app: AppHandle) {
    spawn(app.clone(), "integration_n8n", 3, 15, poll_n8n);
    spawn(app.clone(), "integration_vercel", 5, 30, poll_vercel);
    spawn(app.clone(), "integration_stripe", 6, 30, poll_stripe);
    spawn(app.clone(), "integration_resend", 6, 60, poll_resend);
    spawn(app.clone(), "integration_github", 7, 300, poll_github);
    spawn(app.clone(), "integration_gitlab", 8, 60, poll_gitlab);
    spawn(app.clone(), "integration_jenkins", 4, 15, poll_jenkins);
    spawn(app.clone(), "integration_calcom", 8, 300, poll_calcom);
    spawn(app, "integration_notion", 9, 300, poll_notion);
}

/// True when the user has this integration switched on in settings.
fn enabled(app: &AppHandle, id: &str) -> bool {
    app.try_state::<crate::Shared>()
        .map(|shared| {
            let settings = shared.settings.lock().unwrap();
            settings.active_integrations.iter().any(|x| x == id)
        })
        .unwrap_or(false)
}

fn spawn<F, Fut>(app: AppHandle, id: &'static str, delay_secs: u64, every_secs: u64, poll: F)
where
    F: Fn(AppHandle) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(every_secs));
        loop {
            ticker.tick().await;
            // The ticker keeps its cadence; we just decline to do the work. An
            // integration the user switched off, or a paused app, must make no
            // network calls at all — CLAUDE.md allows talking only to services
            // the user configured, and a disabled one is not configured.
            if PAUSED.load(Ordering::Relaxed) || !enabled(&app, id) {
                continue;
            }
            poll(app.clone()).await;
        }
    });
}

/// One-shot refresh from the Refresh buttons in the island.
pub async fn poll_once(app: AppHandle, id: &str) {
    match id {
        "integration_stripe" => poll_stripe(app).await,
        "integration_github" => poll_github(app).await,
        "integration_gitlab" => poll_gitlab(app).await,
        "integration_jenkins" => poll_jenkins(app).await,
        "integration_vercel" => poll_vercel(app).await,
        "integration_n8n" => poll_n8n(app).await,
        "integration_resend" => poll_resend(app).await,
        "integration_notion" => poll_notion(app).await,
        "integration_calcom" => poll_calcom(app).await,
        _ => {}
    }
}

/// Remembers the newest id per integration so an event fires once, not on every poll.
struct Seen(Mutex<std::collections::HashMap<&'static str, String>>);

static SEEN: std::sync::LazyLock<Seen> =
    std::sync::LazyLock::new(|| Seen(Mutex::new(std::collections::HashMap::new())));

/// Returns true the first time a given id is seen (and false on the very first
/// load, which only fills the card).
fn is_new(key: &'static str, id: &str) -> bool {
    let mut map = SEEN.0.lock().unwrap();
    match map.insert(key, id.to_string()) {
        Some(previous) => previous != id,
        None => false, // first poll: populate silently, like the Swift pollers
    }
}

fn status_error(code: u16, unauthorised_hint: &str) -> String {
    match code {
        401 => "Invalid API key (401)".into(),
        403 => unauthorised_hint.into(),
        _ => format!("API error {code}"),
    }
}

// ── Stripe ────────────────────────────────────────────────────────────────────

async fn poll_stripe(app: AppHandle) {
    let Some(key) = secrets::get("stripe-api-key") else { return };
    let auth = format!("Basic {}", crate::claude::base64_for(format!("{key}:").as_bytes()));
    let http = client();

    let balance = http
        .get("https://api.stripe.com/v1/balance")
        .header("Authorization", &auth)
        .send()
        .await;

    let (amount, currency) = match balance {
        Ok(r) if r.status().is_success() => {
            let json: Value = r.json().await.unwrap_or(json!({}));
            let mut buckets: Vec<Value> = Vec::new();
            for k in ["available", "pending"] {
                if let Some(arr) = json.get(k).and_then(Value::as_array) {
                    buckets.extend(arr.iter().cloned());
                }
            }
            let currency = buckets
                .first()
                .and_then(|b| b.get("currency"))
                .and_then(Value::as_str)
                .unwrap_or("eur")
                .to_string();
            let amount: i64 = buckets
                .iter()
                .filter_map(|b| b.get("amount").and_then(Value::as_i64))
                .sum();
            (amount, currency)
        }
        Ok(r) => {
            let code = r.status().as_u16();
            emit(&app, IntegrationUpdate {
                id: "integration_stripe",
                data: json!({}),
                error: Some(status_error(code, "Use a secret key (sk_live_… not pk_live_…)")),
                event: None,
            });
            return;
        }
        Err(e) => {
            emit(&app, IntegrationUpdate {
                id: "integration_stripe",
                data: json!({}),
                error: Some(format!("No connection: {e}")),
                event: None,
            });
            return;
        }
    };

    let charges = http
        .get("https://api.stripe.com/v1/charges?limit=3")
        .header("Authorization", &auth)
        .send()
        .await;
    let Ok(response) = charges else { return };
    if !response.status().is_success() {
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let payments: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let description = c
                        .get("description")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            c.get("billing_details")
                                .and_then(|b| b.get("name"))
                                .and_then(Value::as_str)
                        })
                        .map(str::to_string);
                    Some(json!({
                        "id": c.get("id")?.as_str()?,
                        "amount": c.get("amount")?.as_i64()?,
                        "currency": c.get("currency")?.as_str()?,
                        "description": description,
                        "createdAt": c.get("created").and_then(Value::as_i64).unwrap_or(0) * 1000,
                        "status": c.get("status").and_then(Value::as_str).unwrap_or("succeeded"),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let newest = payments
        .first()
        .and_then(|p| p.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let event = if !newest.is_empty() && is_new("stripe", &newest) {
        let label = payments[0]
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                let cents = payments[0].get("amount").and_then(Value::as_i64).unwrap_or(0);
                format!("{:.2}", cents as f64 / 100.0)
            });
        Some(IntegrationEvent { success: true, label, detail: None, phase: None })
    } else {
        None
    };

    emit(&app, IntegrationUpdate {
        id: "integration_stripe",
        data: json!({ "balance": amount, "currency": currency, "payments": payments }),
        error: None,
        event,
    });
}

// ── GitHub ────────────────────────────────────────────────────────────────────

async fn poll_github(app: AppHandle) {
    let Some(token) = secrets::get("github-token") else { return };
    let http = client();

    let user = http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Coucou")
        .send()
        .await;
    let Ok(response) = user else { return };
    if !response.status().is_success() {
        emit(&app, IntegrationUpdate {
            id: "integration_github",
            data: json!({}),
            error: Some(status_error(response.status().as_u16(), "Token lacks the needed scope")),
            event: None,
        });
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let public = json.get("public_repos").and_then(Value::as_i64).unwrap_or(0);
    let private = json
        .get("owned_private_repos")
        .or_else(|| json.get("total_private_repos"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let repos = http
        .get("https://api.github.com/user/repos?per_page=100&affiliation=owner&sort=pushed")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Coucou")
        .send()
        .await;
    let stars: i64 = match repos {
        Ok(r) if r.status().is_success() => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .map(|list| {
                list.iter()
                    .filter_map(|r| r.get("stargazers_count").and_then(Value::as_i64))
                    .sum()
            })
            .unwrap_or(0),
        _ => 0,
    };

    emit(&app, IntegrationUpdate {
        id: "integration_github",
        data: json!({ "totalRepos": public + private, "totalStars": stars }),
        error: None,
        event: None,
    });
}

// ── GitLab ────────────────────────────────────────────────────────────────────
//
// One instance, whichever URL the user stored. Events cover pushes, commits,
// issues and comments across the projects they can see. Merge requests come
// from their own endpoint: the events API does not return them.

static GITLAB_PROJECTS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

struct GitlabItem {
    /// Identity used to notice a new event. Merge requests include their update time.
    seen: String,
    feed: &'static str,
    kind: &'static str,
    title: String,
    project_id: Option<i64>,
    project: String,
    author: String,
    reference: String,
    iid: Option<i64>,
    created_at: String,
    failure: bool,
    tail: Option<String>,
}

fn gitlab_error(app: &AppHandle, error: String) {
    emit(app, IntegrationUpdate {
        id: "integration_gitlab",
        data: json!({}),
        error: Some(error),
        event: None,
    });
}

fn gitlab_http() -> reqwest::Client {
    // No redirects: the token must stay on the instance the user typed.
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// `https://gitlab.example.com`, or the same host with a subpath. `/api/v4` is stripped.
fn gitlab_base(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("Instance URL missing".into());
    }
    let url = reqwest::Url::parse(raw).map_err(|_| "Instance URL is not a URL".to_string())?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err("Instance URL must start with http:// or https://".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Put the token in the token field, not in the URL".into());
    }
    if url.host_str().is_none() {
        return Err("Instance URL has no host".into());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    if let Some(stripped) = path.strip_suffix("/api/v4") {
        path = stripped.trim_end_matches('/').to_string();
    }
    let mut base = url.origin().ascii_serialization();
    if !path.is_empty() && path != "/" {
        base.push_str(&path);
    }
    Ok(base)
}

fn json_i64(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| v.as_i64().or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok())))
}

fn json_str<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn clip_line(raw: &str) -> String {
    let flat: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (i, ch) in flat.chars().enumerate() {
        if i == 90 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn safe_project_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains("..")
        && !path.contains("//")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn safe_ref(reference: &str) -> bool {
    !reference.is_empty()
        && !reference.contains("..")
        && !reference.starts_with('/')
        && !reference.starts_with('-')
        && reference
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn safe_sha(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn tail_is_safe(tail: &str) -> bool {
    tail.starts_with("/-/")
        && !tail.contains("..")
        && tail
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn issue_tail(iid: i64) -> Option<String> {
    (iid > 0).then(|| format!("/-/issues/{iid}"))
}

fn merge_tail(iid: i64) -> Option<String> {
    (iid > 0).then(|| format!("/-/merge_requests/{iid}"))
}

fn push_tail(push: &Value) -> Option<String> {
    let sha = push.get("commit_to").and_then(Value::as_str).unwrap_or("");
    let reference = push.get("ref").and_then(Value::as_str).unwrap_or("");
    let count = push.get("commit_count").and_then(Value::as_i64).unwrap_or(0);
    if count <= 1 && safe_sha(sha) {
        return Some(format!("/-/commit/{sha}"));
    }
    if safe_ref(reference) {
        let folder = if push.get("ref_type").and_then(Value::as_str) == Some("tag") {
            "tags"
        } else {
            "commits"
        };
        return Some(format!("/-/{folder}/{reference}"));
    }
    None
}

fn gitlab_url(base: &str, project: &str, tail: &Option<String>) -> Option<String> {
    if !safe_project_path(project) {
        return None;
    }
    match tail {
        Some(tail) if tail_is_safe(tail) => Some(format!("{base}/{project}{tail}")),
        _ => Some(format!("{base}/{project}")),
    }
}

fn author_name(v: &Value) -> String {
    let direct = json_str(v, "author_username");
    if !direct.is_empty() {
        return direct.to_string();
    }
    v.get("author")
        .and_then(|a| a.get("username"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn map_gitlab_event(v: &Value) -> Option<GitlabItem> {
    let id = json_i64(v.get("id"))?;
    let action = json_str(v, "action_name");
    if matches!(action, "joined" | "left" | "expired") {
        return None;
    }
    let target_type = json_str(v, "target_type");
    let project_id = json_i64(v.get("project_id"));
    let created_at = json_str(v, "created_at").to_string();
    let author = author_name(v);
    let iid = json_i64(v.get("target_iid")).filter(|n| *n > 0);
    let failure = action.to_ascii_lowercase().contains("fail");

    if target_type.eq_ignore_ascii_case("Note") {
        let note = v.get("note")?;
        if note.get("system").and_then(Value::as_bool).unwrap_or(false) {
            return None;
        }
        let body = clip_line(note.get("body").and_then(Value::as_str).unwrap_or(""));
        let title = if body.is_empty() {
            clip_line(json_str(v, "target_title"))
        } else {
            body
        };
        let noteable = note.get("noteable_type").and_then(Value::as_str).unwrap_or("");
        let note_iid = json_i64(note.get("noteable_iid")).filter(|n| *n > 0);
        let tail = match noteable {
            "Issue" => note_iid.and_then(issue_tail),
            "MergeRequest" => note_iid.and_then(merge_tail),
            _ => None,
        };
        return Some(GitlabItem {
            seen: id.to_string(),
            feed: "event",
            kind: "note",
            title,
            project_id,
            project: String::new(),
            author,
            reference: String::new(),
            iid: note_iid.or(iid),
            created_at,
            failure: false,
            tail,
        });
    }

    let pushed = action.to_ascii_lowercase().contains("push");
    if pushed || v.get("push_data").is_some_and(Value::is_object) {
        let push = v.get("push_data").filter(|p| p.is_object());
        let reference = push
            .and_then(|p| p.get("ref"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let title = clip_line(
            push.and_then(|p| p.get("commit_title"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        return Some(GitlabItem {
            seen: id.to_string(),
            feed: "event",
            kind: "push",
            title,
            project_id,
            project: String::new(),
            author,
            reference,
            iid: None,
            created_at,
            failure,
            tail: push.and_then(push_tail),
        });
    }

    let kind = if target_type.eq_ignore_ascii_case("Issue") {
        "issue"
    } else if target_type.eq_ignore_ascii_case("MergeRequest") {
        "merge"
    } else {
        "other"
    };
    let tail = match kind {
        "issue" => iid.and_then(issue_tail),
        "merge" => iid.and_then(merge_tail),
        _ => None,
    };
    Some(GitlabItem {
        seen: id.to_string(),
        feed: "event",
        kind,
        title: clip_line(json_str(v, "target_title")),
        project_id,
        project: String::new(),
        author,
        reference: String::new(),
        iid,
        created_at,
        failure,
        tail,
    })
}

fn map_gitlab_merge(v: &Value) -> Option<GitlabItem> {
    let id = json_i64(v.get("id"))?;
    let iid = json_i64(v.get("iid")).filter(|n| *n > 0)?;
    let updated = json_str(v, "updated_at");
    if updated.is_empty() {
        return None;
    }
    Some(GitlabItem {
        seen: format!("{id}:{updated}"),
        feed: "mr",
        kind: "merge",
        title: clip_line(json_str(v, "title")),
        project_id: json_i64(v.get("project_id")),
        project: String::new(),
        author: v
            .get("author")
            .and_then(|a| a.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        reference: json_str(v, "source_branch").to_string(),
        iid: Some(iid),
        created_at: updated.to_string(),
        failure: false,
        tail: merge_tail(iid),
    })
}

fn gitlab_json(base: &str, item: &GitlabItem) -> Value {
    json!({
        "id": item.seen,
        "kind": item.kind,
        "title": item.title,
        "project": item.project,
        "author": item.author,
        "ref": item.reference,
        "iid": item.iid,
        "createdAt": item.created_at,
        "url": gitlab_url(base, &item.project, &item.tail),
        "failure": item.failure,
    })
}

fn gitlab_alert(item: &GitlabItem) -> (String, Option<String>) {
    let short = item
        .project
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("GitLab");
    let label = if item.author.is_empty() {
        short.to_string()
    } else {
        format!("{} · {short}", item.author)
    };
    let detail = if item.title.is_empty() { None } else { Some(item.title.clone()) };
    (label, detail)
}

async fn gitlab_project(http: &reqwest::Client, base: &str, token: &str, id: i64) -> Option<String> {
    let key = format!("{base}|{id}");
    if let Some(hit) = GITLAB_PROJECTS.lock().ok().and_then(|map| map.get(&key).cloned()) {
        return Some(hit);
    }
    let response = http
        .get(format!("{base}/api/v4/projects/{id}"))
        .header("PRIVATE-TOKEN", token)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: Value = response.json().await.ok()?;
    let path = json.get("path_with_namespace")?.as_str()?.to_string();
    if !safe_project_path(&path) {
        return None;
    }
    if let Ok(mut map) = GITLAB_PROJECTS.lock() {
        if map.len() > 200 {
            map.clear();
        }
        map.insert(key, path.clone());
    }
    Some(path)
}

async fn poll_gitlab(app: AppHandle) {
    let Some(token) = secrets::get("gitlab-token") else { return };
    let Some(raw_base) = secrets::get("gitlab-url") else {
        gitlab_error(&app, "Instance URL missing".into());
        return;
    };
    let base = match gitlab_base(&raw_base) {
        Ok(base) => base,
        Err(err) => {
            gitlab_error(&app, err);
            return;
        }
    };
    let http = gitlab_http();
    let response = match http
        .get(format!("{base}/api/v4/events?scope=all&sort=desc&per_page=20"))
        .header("PRIVATE-TOKEN", &token)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            gitlab_error(&app, format!("No connection: {err}"));
            return;
        }
    };
    let status = response.status();
    if status.is_redirection() {
        gitlab_error(&app, "GitLab redirected the request".into());
        return;
    }
    if !status.is_success() {
        log::line(format!("gitlab events HTTP {}", status.as_u16()));
        gitlab_error(&app, status_error(status.as_u16(), "Token needs the read_api scope"));
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!([]));
    let Value::Array(raw_events) = json else {
        gitlab_error(&app, "Unexpected GitLab response".into());
        return;
    };
    let mut items: Vec<GitlabItem> = raw_events.iter().filter_map(map_gitlab_event).collect();

    if let Ok(response) = http
        .get(format!(
            "{base}/api/v4/merge_requests?scope=all&state=all&order_by=updated_at&sort=desc&per_page=10"
        ))
        .header("PRIVATE-TOKEN", &token)
        .header("Accept", "application/json")
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(Value::Array(list)) = response.json::<Value>().await {
                items.extend(list.iter().filter_map(map_gitlab_merge));
            }
        } else {
            log::line(format!("gitlab merge requests HTTP {}", response.status()));
        }
    }

    let mut project_ids: Vec<i64> = items.iter().filter_map(|item| item.project_id).collect();
    project_ids.sort_unstable();
    project_ids.dedup();
    project_ids.truncate(8);
    let mut names = std::collections::HashMap::new();
    for id in project_ids {
        if let Some(path) = gitlab_project(&http, &base, &token, id).await {
            names.insert(id, path);
        }
    }
    for item in &mut items {
        if let Some(id) = item.project_id {
            if let Some(path) = names.get(&id) {
                item.project = path.clone();
            }
        }
    }

    let event_seen = items
        .iter()
        .filter(|item| item.feed == "event")
        .max_by_key(|item| item.seen.parse::<i64>().unwrap_or(0))
        .map(|item| item.seen.clone());
    let mr_seen = items
        .iter()
        .filter(|item| item.feed == "mr")
        .max_by(|a, b| a.created_at.cmp(&b.created_at))
        .map(|item| item.seen.clone());
    let event_new = event_seen.as_deref().is_some_and(|id| is_new("gitlab-event", id));
    let mr_new = mr_seen.as_deref().is_some_and(|id| is_new("gitlab-mr", id));

    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let alert = items.iter().find(|item| {
        (event_new && item.feed == "event" && event_seen.as_deref() == Some(item.seen.as_str()))
            || (mr_new && item.feed == "mr" && mr_seen.as_deref() == Some(item.seen.as_str()))
    });
    let event = alert.map(|item| {
        let (label, detail) = gitlab_alert(item);
        IntegrationEvent { success: !item.failure, label, detail, phase: None }
    });
    let shown: Vec<Value> = items.iter().take(8).map(|item| gitlab_json(&base, item)).collect();
    log::line(format!("gitlab {} event(s)", shown.len()));

    emit(&app, IntegrationUpdate {
        id: "integration_gitlab",
        data: json!({ "webUrl": base, "events": shown }),
        error: None,
        event,
    });
}

// ── GitLab browse ─────────────────────────────────────────────────────────────
//
// On-demand reads for the full-width browser: projects, commits, one commit's
// full message, pipelines and their jobs. The token stays in the credential
// store and is only sent to the instance URL the user configured.

const BROWSE_PAGE: u32 = 40;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitlabBrowseRequest {
    pub kind: String,
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub pipeline_id: Option<i64>,
    #[serde(default)]
    pub page: Option<u32>,
}

pub async fn gitlab_browse(req: GitlabBrowseRequest) -> Result<Value, String> {
    let token = secrets::get("gitlab-token").ok_or_else(|| "GitLab token missing".to_string())?;
    let raw = secrets::get("gitlab-url").ok_or_else(|| "Instance URL missing".to_string())?;
    let base = gitlab_base(&raw)?;
    let http = gitlab_http();
    let page = req.page.unwrap_or(1).clamp(1, 10);
    match req.kind.as_str() {
        "projects" => gitlab_list(&http, &token, &format!(
            "{base}/api/v4/projects?membership=true&simple=true&archived=false&order_by=last_activity_at&sort=desc&per_page={BROWSE_PAGE}&page={page}"
        ), |item| map_gitlab_project(item, &base)).await,
        "commits" => {
            let id = positive_id(req.project_id)?;
            gitlab_list(&http, &token, &format!(
                "{base}/api/v4/projects/{id}/repository/commits?per_page={BROWSE_PAGE}&page={page}"
            ), |item| map_gitlab_commit(item, &base)).await
        }
        "pipelines" => {
            let id = positive_id(req.project_id)?;
            gitlab_list(&http, &token, &format!(
                "{base}/api/v4/projects/{id}/pipelines?order_by=updated_at&sort=desc&per_page={BROWSE_PAGE}&page={page}"
            ), |item| map_gitlab_pipeline(item, &base)).await
        }
        "jobs" => {
            let id = positive_id(req.project_id)?;
            let pipeline = positive_id(req.pipeline_id)?;
            gitlab_list(&http, &token, &format!(
                "{base}/api/v4/projects/{id}/pipelines/{pipeline}/jobs?per_page=100"
            ), |item| map_gitlab_job(item, &base)).await
        }
        _ => Err("Unknown GitLab screen".into()),
    }
}

fn positive_id(id: Option<i64>) -> Result<i64, String> {
    match id {
        Some(id) if id > 0 => Ok(id),
        _ => Err("Missing GitLab id".into()),
    }
}

async fn gitlab_list(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    map: impl Fn(&Value) -> Option<Value>,
) -> Result<Value, String> {
    let response = http
        .get(url)
        .header("PRIVATE-TOKEN", token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("No connection: {err}"))?;
    let status = response.status();
    if status.is_redirection() {
        return Err("GitLab redirected the request".into());
    }
    if !status.is_success() {
        log::line(format!("gitlab browse HTTP {}", status.as_u16()));
        return Err(status_error(status.as_u16(), "Token needs the read_api scope"));
    }
    let json: Value = response.json().await.map_err(|_| "Unexpected GitLab response".to_string())?;
    let Value::Array(items) = json else {
        return Err("Unexpected GitLab response".into());
    };
    Ok(Value::Array(items.iter().filter_map(map).collect()))
}

/// Text safe to show. No control characters, capped so one field cannot fill the island.
fn plain(raw: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    for ch in raw.chars() {
        if n >= max {
            break;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
        n += 1;
    }
    out
}

fn hosted_url(base: &str, url: &str) -> Option<String> {
    let rest = url.strip_prefix(base)?;
    if !rest.is_empty() && !rest.starts_with('/') && !rest.starts_with('?') {
        return None;
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(url.to_string())
}

fn map_gitlab_project(v: &Value, base: &str) -> Option<Value> {
    let id = json_i64(v.get("id")).filter(|id| *id > 0)?;
    let path = plain(json_str(v, "path_with_namespace"), 240);
    if path.is_empty() {
        return None;
    }
    let name = plain(json_str(v, "name"), 120);
    let web = v.get("web_url").and_then(Value::as_str).and_then(|url| hosted_url(base, url));
    let url = web.or_else(|| gitlab_url(base, &path, &None));
    Some(json!({
        "id": id,
        "name": if name.is_empty() { path.clone() } else { name },
        "path": path,
        "branch": plain(json_str(v, "default_branch"), 120),
        "activityAt": plain(json_str(v, "last_activity_at"), 40),
        "url": url,
    }))
}

fn map_gitlab_commit(v: &Value, base: &str) -> Option<Value> {
    let sha = json_str(v, "id");
    if !safe_sha(sha) {
        return None;
    }
    let title = plain(json_str(v, "title"), 500);
    let message = plain(json_str(v, "message"), 8000);
    let author = plain(json_str(v, "author_name"), 200);
    let email = plain_email(json_str(v, "author_email"));
    let web = v.get("web_url").and_then(Value::as_str).and_then(|url| hosted_url(base, url));
    Some(json!({
        "sha": sha,
        "shortId": &sha[..8],
        "title": if title.is_empty() { message.lines().next().unwrap_or("").to_string() } else { title },
        "message": message,
        "author": author,
        "email": email,
        "createdAt": plain(json_str(v, "authored_date"), 40),
        "url": web,
    }))
}

fn plain_email(raw: &str) -> String {
    let email = plain(raw, 120);
    if email.contains(' ') || email.contains('<') || !email.contains('@') {
        return String::new();
    }
    email
}

fn map_gitlab_pipeline(v: &Value, base: &str) -> Option<Value> {
    let id = json_i64(v.get("id")).filter(|id| *id > 0)?;
    let sha = json_str(v, "sha");
    let short = if safe_sha(sha) { sha[..8].to_string() } else { String::new() };
    let web = v.get("web_url").and_then(Value::as_str).and_then(|url| hosted_url(base, url));
    Some(json!({
        "id": id,
        "status": pipeline_status(json_str(v, "status")),
        "ref": plain(json_str(v, "ref"), 160),
        "sha": short,
        "createdAt": plain(json_str(v, "created_at"), 40),
        "url": web,
    }))
}

fn pipeline_status(raw: &str) -> &'static str {
    match raw {
        "created" => "created",
        "waiting_for_resource" => "waiting_for_resource",
        "preparing" => "preparing",
        "pending" => "pending",
        "running" => "running",
        "success" => "success",
        "failed" => "failed",
        "canceled" => "canceled",
        "canceling" => "canceling",
        "skipped" => "skipped",
        "manual" => "manual",
        "scheduled" => "scheduled",
        _ => "other",
    }
}

fn map_gitlab_job(v: &Value, base: &str) -> Option<Value> {
    let id = json_i64(v.get("id")).filter(|id| *id > 0)?;
    let name = plain(json_str(v, "name"), 160);
    if name.is_empty() {
        return None;
    }
    let web = v.get("web_url").and_then(Value::as_str).and_then(|url| hosted_url(base, url));
    Some(json!({
        "id": id,
        "name": name,
        "stage": plain(json_str(v, "stage"), 80),
        "status": pipeline_status(json_str(v, "status")),
        "url": web,
    }))
}

// ── Jenkins ───────────────────────────────────────────────────────────────────
//
// One instance, whichever URL the user stored. Every 15 s Coucou reads the job
// tree and the queue. A build alerts once per transition: queued, started,
// success, failure, unstable, aborted. The first poll only fills the card.

const JENKINS_RECENT_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone)]
struct JenkinsRow {
    key: String,
    signature: String,
    phase: &'static str,
    name: String,
    number: Option<i64>,
    url: String,
    timestamp: i64,
}

struct JenkinsMemory {
    base: String,
    primed: bool,
    jobs: std::collections::HashMap<String, String>,
}

static JENKINS_MEMORY: std::sync::LazyLock<Mutex<JenkinsMemory>> = std::sync::LazyLock::new(|| {
    Mutex::new(JenkinsMemory {
        base: String::new(),
        primed: false,
        jobs: std::collections::HashMap::new(),
    })
});

fn jenkins_error(app: &AppHandle, error: String) {
    emit(app, IntegrationUpdate {
        id: "integration_jenkins",
        data: json!({}),
        error: Some(error),
        event: None,
    });
}

fn jenkins_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// `https://jenkins.example.com` or `https://ci.example.com/jenkins`. `/api` is stripped.
fn jenkins_base(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("Instance URL missing".into());
    }
    let url = reqwest::Url::parse(raw).map_err(|_| "Instance URL is not a URL".to_string())?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err("Instance URL must start with http:// or https://".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Put the token in the token field, not in the URL".into());
    }
    if url.host_str().is_none() {
        return Err("Instance URL has no host".into());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in ["/api/json", "/login", "/api"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            path = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    let mut base = url.origin().ascii_serialization();
    if !path.is_empty() && path != "/" {
        base.push_str(&path);
    }
    Ok(base)
}

fn jenkins_jobs_tree(depth: usize) -> String {
    let fields =
        "name,fullName,url,color,lastBuild[number,url,result,building,timestamp,duration,fullDisplayName]";
    if depth == 0 {
        return fields.to_string();
    }
    format!("{fields},jobs[{}]", jenkins_jobs_tree(depth - 1))
}

fn jenkins_phase(build: &Value) -> &'static str {
    if build.get("building").and_then(Value::as_bool) == Some(true) {
        return "building";
    }
    match build.get("result").and_then(Value::as_str).unwrap_or("") {
        "SUCCESS" => "success",
        "FAILURE" => "failure",
        "UNSTABLE" => "unstable",
        "ABORTED" => "aborted",
        "NOT_BUILT" => "notbuilt",
        _ => "unknown",
    }
}

fn jenkins_alert_phase(phase: &str) -> Option<&'static str> {
    match phase {
        "building" | "queued" => Some("working"),
        "success" => Some("finished"),
        "failure" | "unstable" | "aborted" => Some("error"),
        _ => None,
    }
}

fn jenkins_keep(phase: &str, timestamp: i64, now: i64) -> bool {
    if matches!(phase, "unknown" | "notbuilt") {
        return false;
    }
    if matches!(phase, "building" | "queued") {
        return true;
    }
    timestamp > 0 && now.saturating_sub(timestamp) <= JENKINS_RECENT_MS
}

fn jenkins_short(name: &str) -> String {
    name.rsplit(['/', '»'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_string()
}

fn allowed_jenkins_url(base: &str, raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let base = reqwest::Url::parse(base).ok()?;
    let url = reqwest::Url::parse(raw).ok()?;
    if url.scheme() != base.scheme()
        || url.host() != base.host()
        || url.port_or_known_default() != base.port_or_known_default()
    {
        return None;
    }
    Some(url.to_string())
}

fn map_jenkins_job(job: &Value) -> Option<JenkinsRow> {
    let build = job.get("lastBuild")?;
    if !build.is_object() {
        return None;
    }
    let number = json_i64(build.get("number")).filter(|n| *n > 0)?;
    let phase = jenkins_phase(build);
    let name = job
        .get("fullName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| job.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()))?;
    let timestamp = json_i64(build.get("timestamp")).unwrap_or(0);
    Some(JenkinsRow {
        key: name.to_string(),
        signature: format!("{number}:{phase}"),
        phase,
        name: name.to_string(),
        number: Some(number),
        url: build.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
        timestamp,
    })
}

fn map_jenkins_queue(item: &Value) -> Option<JenkinsRow> {
    let id = json_i64(item.get("id")).filter(|n| *n > 0)?;
    let task = item.get("task")?;
    let name = task
        .get("fullDisplayName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| task.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()))?;
    Some(JenkinsRow {
        key: format!("queue:{id}"),
        signature: "queued".into(),
        phase: "queued",
        name: name.to_string(),
        number: None,
        url: task.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
        timestamp: json_i64(item.get("inQueueSince")).unwrap_or(0),
    })
}

fn collect_jenkins_jobs(node: &Value, out: &mut Vec<JenkinsRow>) {
    let Some(jobs) = node.get("jobs").and_then(Value::as_array) else { return };
    for job in jobs {
        if let Some(row) = map_jenkins_job(job) {
            out.push(row);
        }
        collect_jenkins_jobs(job, out);
    }
}

fn dedup_jenkins(rows: Vec<JenkinsRow>) -> Vec<JenkinsRow> {
    let mut index = std::collections::HashMap::<String, usize>::new();
    let mut out: Vec<JenkinsRow> = Vec::new();
    for row in rows {
        let number = row.number.unwrap_or(0);
        if let Some(&at) = index.get(&row.key) {
            if number >= out[at].number.unwrap_or(0) {
                out[at] = row;
            }
        } else {
            index.insert(row.key.clone(), out.len());
            out.push(row);
        }
    }
    out
}

fn jenkins_changes(
    primed: bool,
    previous: &std::collections::HashMap<String, String>,
    rows: &[JenkinsRow],
) -> Vec<usize> {
    if !primed {
        return Vec::new();
    }
    let mut idxs: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            jenkins_alert_phase(row.phase).is_some()
                && previous.get(&row.key).map(String::as_str) != Some(row.signature.as_str())
        })
        .map(|(i, _)| i)
        .collect();
    idxs.sort_by_key(|&i| rows[i].timestamp);
    if idxs.len() > 8 {
        idxs = idxs.split_off(idxs.len() - 8);
    }
    idxs
}

fn jenkins_json(base: &str, row: &JenkinsRow) -> Value {
    json!({
        "name": row.name,
        "number": row.number,
        "phase": row.phase,
        "timestamp": row.timestamp,
        "url": allowed_jenkins_url(base, &row.url),
    })
}

fn jenkins_event(row: &JenkinsRow) -> Option<IntegrationEvent> {
    let phase = jenkins_alert_phase(row.phase)?;
    let label = jenkins_short(&row.name);
    let label = if label.is_empty() { "Jenkins".to_string() } else { label };
    let status = crate::i18n::t(&format!("jenkins.{}", row.phase));
    let detail = match row.number {
        Some(n) => Some(format!("#{n} · {status}")),
        None => Some(status),
    };
    Some(IntegrationEvent {
        success: phase == "finished",
        label,
        detail,
        phase: Some(phase),
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

async fn jenkins_get(http: &reqwest::Client, url: &str, auth: &str, tree: &str) -> Result<Value, String> {
    let response = http
        .get(url)
        .query(&[("tree", tree)])
        .header("Authorization", auth)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("No connection: {err}"))?;
    let status = response.status();
    if status.is_redirection() {
        return Err("Jenkins redirected the request".into());
    }
    if !status.is_success() {
        log::line(format!("jenkins HTTP {}", status.as_u16()));
        return Err(status_error(
            status.as_u16(),
            "Token needs Overall/Read and Job/Read",
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Unexpected Jenkins response".to_string())
}

async fn poll_jenkins(app: AppHandle) {
    let Some(token) = secrets::get("jenkins-token") else { return };
    let Some(raw_base) = secrets::get("jenkins-url") else {
        jenkins_error(&app, "Instance URL missing".into());
        return;
    };
    let Some(user) = secrets::get("jenkins-user") else {
        jenkins_error(&app, "Jenkins user missing".into());
        return;
    };
    let base = match jenkins_base(&raw_base) {
        Ok(base) => base,
        Err(err) => {
            jenkins_error(&app, err);
            return;
        }
    };
    let auth = format!(
        "Basic {}",
        crate::claude::base64_for(format!("{user}:{token}").as_bytes())
    );
    let http = jenkins_http();
    let tree = format!("jobs[{}]", jenkins_jobs_tree(3));
    let root = match jenkins_get(&http, &format!("{base}/api/json"), &auth, &tree).await {
        Ok(root) => root,
        Err(err) => {
            jenkins_error(&app, err);
            return;
        }
    };

    let mut rows = Vec::new();
    collect_jenkins_jobs(&root, &mut rows);
    if let Ok(queue) = jenkins_get(&http, &format!("{base}/queue/api/json"), &auth, "items[id,why,inQueueSince,task[name,fullDisplayName,url]]").await {
        if let Some(items) = queue.get("items").and_then(Value::as_array) {
            rows.extend(items.iter().filter_map(map_jenkins_queue));
        }
    } else {
        log::line("jenkins queue skipped");
    }

    let now = now_ms();
    rows.retain(|row| jenkins_keep(row.phase, row.timestamp, now));
    let mut rows = dedup_jenkins(rows);
    rows.sort_by(|a, b| {
        let rank = |phase: &str| match phase {
            "building" => 0,
            "queued" => 1,
            _ => 2,
        };
        rank(a.phase).cmp(&rank(b.phase)).then(b.timestamp.cmp(&a.timestamp))
    });

    let running = rows.iter().filter(|row| matches!(row.phase, "building" | "queued")).count();
    let alerts = {
        let mut memory = JENKINS_MEMORY.lock().unwrap();
        if memory.base != base {
            memory.base = base.clone();
            memory.primed = false;
            memory.jobs.clear();
        }
        let idxs = jenkins_changes(memory.primed, &memory.jobs, &rows);
        memory.jobs.clear();
        for row in &rows {
            memory.jobs.insert(row.key.clone(), row.signature.clone());
        }
        memory.primed = true;
        idxs
    };

    let shown: Vec<Value> = rows.iter().take(12).map(|row| jenkins_json(&base, row)).collect();
    let data = json!({ "webUrl": base, "running": running, "builds": shown });
    log::line(format!("jenkins {} build(s), {} alert(s)", shown.len(), alerts.len()));

    if alerts.is_empty() {
        emit(&app, IntegrationUpdate {
            id: "integration_jenkins",
            data,
            error: None,
            event: None,
        });
        return;
    }
    for index in alerts {
        let Some(event) = jenkins_event(&rows[index]) else { continue };
        emit(&app, IntegrationUpdate {
            id: "integration_jenkins",
            data: data.clone(),
            error: None,
            event: Some(event),
        });
    }
}

// ── Vercel ────────────────────────────────────────────────────────────────────

async fn poll_vercel(app: AppHandle) {
    let Some(token) = secrets::get("vercel-token") else { return };
    let response = client()
        .get("https://api.vercel.com/v6/deployments?limit=5")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await;
    let Ok(response) = response else { return };
    if !response.status().is_success() {
        emit(&app, IntegrationUpdate {
            id: "integration_vercel",
            data: json!({}),
            error: Some(status_error(response.status().as_u16(), "Token lacks access")),
            event: None,
        });
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let terminal = ["READY", "ERROR", "CANCELED"];
    let deployments: Vec<Value> = json
        .get("deployments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    let state = d.get("state")?.as_str()?;
                    if !terminal.contains(&state) {
                        return None;
                    }
                    let meta = d.get("meta");
                    let pick = |keys: [&str; 3]| {
                        meta.and_then(|m| keys.iter().find_map(|k| m.get(*k).and_then(Value::as_str)))
                            .map(str::to_string)
                    };
                    Some(json!({
                        "id": d.get("uid")?.as_str()?,
                        "projectName": d.get("name")?.as_str()?,
                        "url": d.get("url").and_then(Value::as_str).unwrap_or(""),
                        "state": state,
                        "createdAt": d.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0),
                        "commitMessage": pick(["githubCommitMessage", "gitlabCommitMessage", "bitbucketCommitMessage"]),
                        "branch": pick(["githubCommitRef", "gitlabCommitRef", "bitbucketBranch"]),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let event = deployments.first().and_then(|latest| {
        let id = latest.get("id")?.as_str()?;
        if !is_new("vercel", id) {
            return None;
        }
        let success = latest.get("state")?.as_str()? == "READY";
        Some(IntegrationEvent {
            success,
            label: latest.get("projectName")?.as_str()?.to_string(),
            detail: None,
            phase: None,
        })
    });

    emit(&app, IntegrationUpdate {
        id: "integration_vercel",
        data: json!({ "deployments": deployments }),
        error: None,
        event,
    });
}

// ── Resend ────────────────────────────────────────────────────────────────────

async fn poll_resend(app: AppHandle) {
    let Some(key) = secrets::get("resend-api-key") else { return };
    let response = client()
        .get("https://api.resend.com/emails?limit=100")
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
        .send()
        .await;
    let Ok(response) = response else { return };
    if !response.status().is_success() {
        emit(&app, IntegrationUpdate {
            id: "integration_resend",
            data: json!({}),
            error: Some(status_error(response.status().as_u16(), "Key lacks access")),
            event: None,
        });
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let total = json
        .get("total")
        .or_else(|| json.get("count"))
        .and_then(Value::as_i64);
    let emails: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .take(5)
                .filter_map(|e| {
                    let to = match e.get("to") {
                        Some(Value::Array(a)) => a.clone(),
                        Some(Value::String(s)) => vec![Value::String(s.clone())],
                        _ => vec![],
                    };
                    Some(json!({
                        "id": e.get("id")?.as_str()?,
                        "to": to,
                        "subject": e.get("subject").and_then(Value::as_str).unwrap_or(""),
                        "createdAt": e.get("created_at").and_then(Value::as_str).unwrap_or(""),
                        "lastEvent": e.get("last_event").and_then(Value::as_str).unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_resend",
        data: json!({ "emails": emails, "total": total }),
        error: None,
        event: None,
    });
}

// ── Notion ────────────────────────────────────────────────────────────────────

async fn poll_notion(app: AppHandle) {
    let Some(token) = secrets::get("notion-api-key") else { return };
    let response = client()
        .post("https://api.notion.com/v1/search")
        .header("Authorization", format!("Bearer {token}"))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&json!({
            "sort": { "direction": "descending", "timestamp": "last_edited_time" },
            "page_size": 3
        }))
        .send()
        .await;
    let Ok(response) = response else { return };
    if !response.status().is_success() {
        emit(&app, IntegrationUpdate {
            id: "integration_notion",
            data: json!({}),
            error: Some(status_error(response.status().as_u16(), "Integration lacks access")),
            event: None,
        });
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let pages: Vec<Value> = json
        .get("results")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_notion_page).collect())
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_notion",
        data: json!({ "pages": pages }),
        error: None,
        event: None,
    });
}

fn parse_notion_page(obj: &Value) -> Option<Value> {
    let id = obj.get("id")?.as_str()?;
    let is_database = obj.get("object").and_then(Value::as_str) == Some("database");

    let mut title = "Untitled".to_string();
    if is_database {
        if let Some(text) = obj
            .get("title")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|t| t.get("plain_text"))
            .and_then(Value::as_str)
        {
            if !text.is_empty() {
                title = text.to_string();
            }
        }
    } else if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for prop in props.values() {
            if prop.get("type").and_then(Value::as_str) != Some("title") {
                continue;
            }
            if let Some(text) = prop
                .get("title")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|t| t.get("plain_text"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    title = text.to_string();
                    break;
                }
            }
        }
    }

    let emoji = obj
        .get("icon")
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("emoji"))
        .and_then(|i| i.get("emoji"))
        .and_then(Value::as_str);

    Some(json!({
        "id": id,
        "title": title,
        "emoji": emoji,
        "lastEditedAt": obj.get("last_edited_time").and_then(Value::as_str)?,
        "url": obj.get("url").and_then(Value::as_str).unwrap_or("https://notion.so"),
    }))
}

// ── Cal.com ───────────────────────────────────────────────────────────────────

async fn poll_calcom(app: AppHandle) {
    let Some(key) = secrets::get("calcom-api-key") else { return };
    let response = client()
        .get("https://api.cal.com/v2/bookings?status=upcoming")
        .header("Authorization", format!("Bearer {key}"))
        .header("cal-api-version", "2024-08-13")
        .send()
        .await;
    let Ok(response) = response else { return };
    if !response.status().is_success() {
        emit(&app, IntegrationUpdate {
            id: "integration_calcom",
            data: json!({}),
            error: Some(status_error(response.status().as_u16(), "Key lacks access")),
            event: None,
        });
        return;
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let bookings: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|b| {
                    let start = b
                        .get("start")
                        .or_else(|| b.get("startTime"))
                        .and_then(Value::as_str)?;
                    let attendee = b.get("attendees").and_then(Value::as_array).and_then(|a| a.first());
                    let notes = b
                        .get("responses")
                        .and_then(|r| r.get("notes"))
                        .and_then(|n| n.get("value"))
                        .and_then(Value::as_str)
                        .or_else(|| b.get("description").and_then(Value::as_str))
                        .filter(|s| !s.is_empty());
                    Some(json!({
                        "id": b.get("id").map(|v| v.to_string()).unwrap_or_default(),
                        "title": b.get("title").and_then(Value::as_str).unwrap_or("Meeting"),
                        "start": start,
                        "status": b.get("status").and_then(Value::as_str).unwrap_or("accepted"),
                        "attendeeName": attendee.and_then(|a| a.get("name")).and_then(Value::as_str),
                        "attendeeEmail": attendee.and_then(|a| a.get("email")).and_then(Value::as_str),
                        "attendeeNotes": notes,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_calcom",
        data: json!({ "bookings": bookings }),
        error: None,
        event: None,
    });
}

// ── n8n ───────────────────────────────────────────────────────────────────────

async fn poll_n8n(app: AppHandle) {
    let (Some(key), Some(raw_base)) = (secrets::get("n8n-api-key"), secrets::get("n8n-url")) else {
        return;
    };
    let base = raw_base.trim_end_matches('/').to_string();
    let http = client();

    // Same two shapes as the Swift poller: the public API first, then /rest.
    let list_urls = [
        format!("{base}/api/v1/executions?limit=1&includeData=false"),
        format!("{base}/rest/executions?limit=1&includeData=false"),
    ];

    let mut items: Option<Vec<Value>> = None;
    for url in &list_urls {
        let Ok(response) = http.get(url).header("X-N8N-API-KEY", &key).header("Accept", "application/json").send().await
        else {
            continue;
        };
        if !response.status().is_success() {
            // Only the status: a self-hosted base URL can carry credentials.
            log::line(format!("n8n list HTTP {}", response.status()));
            continue;
        }
        let Ok(json) = response.json::<Value>().await else { continue };
        items = match &json {
            Value::Object(o) => o.get("data").and_then(Value::as_array).cloned(),
            Value::Array(a) => Some(a.clone()),
            _ => None,
        };
        if items.is_some() {
            break;
        }
    }

    let Some(first) = items.and_then(|list| list.into_iter().next()) else { return };
    let id = match first.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return,
    };

    let status = first.get("status").and_then(Value::as_str).unwrap_or("");
    if !["success", "error", "crashed", "canceled", "failed"].contains(&status) {
        return;
    }
    if !is_new("n8n", &id) {
        return;
    }
    let success = status == "success";

    let detail_urls = [
        format!("{base}/api/v1/executions/{id}?includeData=true"),
        format!("{base}/api/v1/executions/{id}"),
        format!("{base}/rest/executions/{id}?includeData=true"),
        format!("{base}/rest/executions/{id}"),
    ];
    let mut name = "Workflow".to_string();
    let mut detail = None;
    for url in &detail_urls {
        let Ok(response) = http.get(url).header("X-N8N-API-KEY", &key).header("Accept", "application/json").send().await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(json) = response.json::<Value>().await else { continue };
        name = json
            .get("workflowData")
            .and_then(|w| w.get("name"))
            .and_then(Value::as_str)
            .or_else(|| json.get("name").and_then(Value::as_str))
            .unwrap_or("Workflow")
            .to_string();
        detail = n8n_detail(&json, success);
        break;
    }

    log::line(format!("n8n execution {id} {status} · {name}"));
    emit(&app, IntegrationUpdate {
        id: "integration_n8n",
        data: json!({ "workflow": name, "status": status }),
        error: None,
        event: Some(IntegrationEvent { success, label: name, detail, phase: None }),
    });
}

fn n8n_detail(json: &Value, success: bool) -> Option<String> {
    let result = json.get("data")?.get("resultData")?;
    if !success {
        if let Some(error) = result.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            if let Some(node) = error.get("node").and_then(|n| n.get("name")).and_then(Value::as_str) {
                if !node.is_empty() {
                    return Some(format!("{node}\n{message}"));
                }
            }
            return Some(message.to_string());
        }
        let runs = result.get("runData")?.as_object()?;
        for (node, value) in runs {
            if let Some(message) = value
                .as_array()
                .and_then(|a| a.first())
                .and_then(|r| r.get("error"))
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
            {
                return Some(format!("{node}\n{message}"));
            }
        }
        return None;
    }

    let last_node = result.get("lastNodeExecuted")?.as_str()?;
    let items = result
        .get("runData")?
        .get(last_node)?
        .as_array()?
        .first()?
        .get("data")?
        .get("main")?
        .as_array()?
        .first()?
        .as_array()?;
    let count = items.len();
    let header = format!("→ {last_node} · {count} item{}", if count == 1 { "" } else { "s" });

    let fields = items
        .first()
        .and_then(|i| i.get("json"))
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .take(4)
                .map(|(k, v)| format!("{k}: {}", fmt_value(v)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|s| !s.is_empty());

    Some(match fields {
        Some(f) => format!("{header}\n{f}"),
        None => header,
    })
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.chars().take(50).collect(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(_) => "{…}".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod gitlab_tests {
    use super::*;

    #[test]
    fn instance_url_keeps_the_host_and_drops_the_api_suffix() {
        assert_eq!(gitlab_base("https://gitlab.com").as_deref(), Ok("https://gitlab.com"));
        assert_eq!(gitlab_base("https://gitlab.com/api/v4/").as_deref(), Ok("https://gitlab.com"));
        assert_eq!(
            gitlab_base("https://git.example.com/gitlab").as_deref(),
            Ok("https://git.example.com/gitlab")
        );
        assert_eq!(
            gitlab_base("http://127.0.0.1:8929").as_deref(),
            Ok("http://127.0.0.1:8929")
        );
        assert!(gitlab_base("https://user:token@gitlab.com").is_err());
        assert!(gitlab_base("file:///tmp/gitlab").is_err());
    }

    #[test]
    fn push_links_to_the_commit_and_issue_links_to_the_ticket() {
        let push = json!({
            "id": 4,
            "project_id": 15,
            "action_name": "pushed",
            "author_username": "root",
            "created_at": "2015-12-04T10:33:58.089Z",
            "push_data": {
                "commit_count": 1,
                "action": "pushed",
                "ref_type": "branch",
                "commit_to": "c5feabde2d8cd023215af4d2ceeb7a64839fc428",
                "ref": "main",
                "commit_title": "Add simple search"
            }
        });
        let item = map_gitlab_event(&push).unwrap();
        assert_eq!(item.kind, "push");
        assert_eq!(item.title, "Add simple search");
        assert_eq!(
            gitlab_url("https://gitlab.example.com", "group/app", &item.tail).as_deref(),
            Some("https://gitlab.example.com/group/app/-/commit/c5feabde2d8cd023215af4d2ceeb7a64839fc428")
        );

        let issue = json!({
            "id": 1,
            "project_id": 1,
            "action_name": "opened",
            "target_type": "Issue",
            "target_iid": 53,
            "target_title": "Login fails",
            "author_username": "user3",
            "created_at": "2017-02-09T10:43:19.667Z"
        });
        let item = map_gitlab_event(&issue).unwrap();
        assert_eq!(item.kind, "issue");
        assert_eq!(
            gitlab_url("https://gitlab.example.com", "group/app", &item.tail).as_deref(),
            Some("https://gitlab.example.com/group/app/-/issues/53")
        );
    }

    #[test]
    fn membership_noise_and_system_notes_are_dropped() {
        assert!(map_gitlab_event(&json!({"id": 1, "action_name": "joined"})).is_none());
        let note = json!({
            "id": 7,
            "action_name": "commented on",
            "target_type": "Note",
            "note": { "body": "assigned to @root", "system": true, "noteable_type": "Issue", "noteable_iid": 4 }
        });
        assert!(map_gitlab_event(&note).is_none());
    }

    #[test]
    fn browse_keeps_the_full_commit_title_and_drops_foreign_urls() {
        let long = format!("{} stays whole", "x".repeat(100));
        let commit = json!({
            "id": "c5feabde2d8cd023215af4d2ceeb7a64839fc428",
            "title": long,
            "message": format!("{long}\n\nThe body stays with the title."),
            "author_name": "Ada Lovelace",
            "author_email": "ada@example.com",
            "authored_date": "2015-12-04T10:33:58.000Z",
            "web_url": "https://evil.example/commit/c5feabde"
        });
        let item = map_gitlab_commit(&commit, "https://gitlab.example.com").unwrap();
        assert_eq!(item["title"], long);
        assert!(item["message"].as_str().unwrap().contains("The body stays"));
        assert_eq!(item["author"], "Ada Lovelace");
        assert!(item["url"].is_null());

        let project = json!({
            "id": 15,
            "name": "App",
            "path_with_namespace": "group/app",
            "default_branch": "main",
            "last_activity_at": "2015-12-04T10:33:58.089Z",
            "web_url": "https://gitlab.example.com/group/app"
        });
        let row = map_gitlab_project(&project, "https://gitlab.example.com").unwrap();
        assert_eq!(row["path"], "group/app");
        assert_eq!(row["url"], "https://gitlab.example.com/group/app");
        assert!(map_gitlab_commit(&json!({"id": "not-a-sha", "title": "x"}), "https://gitlab.example.com").is_none());
    }
}

#[cfg(test)]
mod jenkins_tests {
    use super::*;

    #[test]
    fn instance_url_keeps_a_context_path_and_drops_the_api_suffix() {
        assert_eq!(
            jenkins_base("https://jenkins.example.com").as_deref(),
            Ok("https://jenkins.example.com")
        );
        assert_eq!(
            jenkins_base("https://ci.example.com/jenkins/api/json").as_deref(),
            Ok("https://ci.example.com/jenkins")
        );
        assert_eq!(
            jenkins_base("http://127.0.0.1:8080/").as_deref(),
            Ok("http://127.0.0.1:8080")
        );
        assert!(jenkins_base("https://user:token@jenkins.example.com").is_err());
        assert!(jenkins_base("file:///tmp/jenkins").is_err());
    }

    #[test]
    fn phase_reads_building_before_the_result() {
        assert_eq!(
            jenkins_phase(&json!({"building": true, "result": null})),
            "building"
        );
        assert_eq!(jenkins_phase(&json!({"building": false, "result": "SUCCESS"})), "success");
        assert_eq!(jenkins_phase(&json!({"building": false, "result": "FAILURE"})), "failure");
        assert_eq!(jenkins_phase(&json!({"building": false, "result": "UNSTABLE"})), "unstable");
        assert_eq!(jenkins_phase(&json!({"building": false, "result": "ABORTED"})), "aborted");
        assert_eq!(jenkins_alert_phase("queued"), Some("working"));
        assert_eq!(jenkins_alert_phase("success"), Some("finished"));
        assert_eq!(jenkins_alert_phase("failure"), Some("error"));
        assert_eq!(jenkins_alert_phase("notbuilt"), None);
    }

    #[test]
    fn nested_folders_keep_the_leaf_build_and_drop_foreign_urls() {
        let root = json!({
            "jobs": [{
                "name": "folder",
                "fullName": "folder",
                "jobs": [{
                    "name": "app",
                    "fullName": "folder/app",
                    "lastBuild": {
                        "number": 12,
                        "building": true,
                        "result": null,
                        "timestamp": 1_700_000_000_000_i64,
                        "url": "https://jenkins.example.com/job/folder/job/app/12/"
                    }
                }]
            }]
        });
        let mut rows = Vec::new();
        collect_jenkins_jobs(&root, &mut rows);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "folder/app");
        assert_eq!(rows[0].phase, "building");
        assert_eq!(rows[0].signature, "12:building");
        assert_eq!(
            allowed_jenkins_url("https://jenkins.example.com", &rows[0].url).as_deref(),
            Some("https://jenkins.example.com/job/folder/job/app/12/")
        );
        assert!(allowed_jenkins_url("https://jenkins.example.com", "https://evil.example/job/1").is_none());
    }

    #[test]
    fn first_poll_is_silent_and_a_result_change_alerts_once() {
        let building = JenkinsRow {
            key: "folder/app".into(),
            signature: "12:building".into(),
            phase: "building",
            name: "folder/app".into(),
            number: Some(12),
            url: String::new(),
            timestamp: 10,
        };
        let mut previous = std::collections::HashMap::new();
        assert!(jenkins_changes(false, &previous, &[building.clone()]).is_empty());
        previous.insert(building.key.clone(), building.signature.clone());

        let success = JenkinsRow {
            signature: "12:success".into(),
            phase: "success",
            timestamp: 20,
            ..building.clone()
        };
        let changed = jenkins_changes(true, &previous, &[success.clone()]);
        assert_eq!(changed, vec![0]);
        let event = jenkins_event(&success).unwrap();
        assert_eq!(event.phase, Some("finished"));
        assert!(event.success);
        assert_eq!(event.label, "app");
        assert!(event.detail.as_deref().unwrap_or("").starts_with("#12"));

        previous.insert(success.key.clone(), success.signature.clone());
        assert!(jenkins_changes(true, &previous, &[success]).is_empty());
    }

    #[test]
    fn old_builds_drop_but_a_running_one_stays() {
        let now = 2_000_000_000_000_i64;
        assert!(jenkins_keep("building", 0, now));
        assert!(jenkins_keep("queued", 0, now));
        assert!(jenkins_keep("success", now - 60_000, now));
        assert!(!jenkins_keep("failure", now - JENKINS_RECENT_MS - 1, now));
        assert!(!jenkins_keep("unknown", now, now));
    }
}
