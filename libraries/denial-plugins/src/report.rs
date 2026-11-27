//! What a program shows of the manager (PLUGIN_MANAGER.md, section 23):
//! what runs, why, what failed and what to offer. Settings displays it and
//! works nothing out itself. Every sentence comes with a code; a program
//! that knows the code words it itself, else it shows the English `text`.

use serde_json::Value;

use crate::activation::BUNDLE_CAPABILITY;
use crate::object;
use crate::progress::job_progress;
use crate::rebuild::is_release;
use crate::store::Store;
use crate::util::{self, Map, Result, obj, text};
use crate::workspace::same_composition;

/// The version of `summary` and of the list. A new field or code keeps it;
/// a changed meaning raises it.
pub const CONTRACT: i64 = 1;

fn line(code: &str, text: &str, facts: Map) -> Value {
    let mut map = object! {"code" => code, "text" => text};
    map.extend(facts);
    Value::Object(map)
}

fn sorted_keys(value: Option<&Value>) -> Vec<String> {
    let mut keys: Vec<String> = obj(value.unwrap_or(&Value::Null)).keys().cloned().collect();
    keys.sort();
    keys
}

/// `denial_top_bar` reads "Top Bar".
pub fn plugin_title(name: &str) -> String {
    name.strip_prefix("denial_")
        .unwrap_or(name)
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_names(catalog: &Map) -> Vec<String> {
    util::list(catalog, "builtins")
        .iter()
        .filter(|plugin| plugin["default"] == true)
        .filter_map(|plugin| plugin["name"].as_str().map(str::to_owned))
        .collect()
}

/// Every plugin this installation knows: built-in ones, ones added before,
/// and the community list's.
pub fn plugin_list(store: &Store, catalog: &Map) -> Result<Vec<Map>> {
    let selection = store.selection()?;
    let selected = obj(&selection["roots"]);
    let installed = store.installed()?;
    let defaults = default_names(catalog);
    let mut plugins: Vec<Map> = Vec::new();
    let mut add = |plugin: &Value, installed: bool| {
        let Some(name) = plugin["name"].as_str() else {
            return;
        };
        if plugins
            .iter()
            .any(|known| text(known, "name") == Some(name))
        {
            return;
        }
        let source = plugin.get("source").cloned().unwrap_or(Value::Null);
        let kind = source.get("kind").cloned().unwrap_or(Value::Null);
        let from = if kind == "builtin" {
            Value::from("built in")
        } else {
            source.get("location").cloned().unwrap_or(Value::Null)
        };
        plugins.push(object! {
            "name" => name,
            "title" => plugin_title(name),
            "description" => plugin.get("description").cloned().unwrap_or(Value::Null),
            // builtin, local or git.
            "kind" => kind,
            "from" => from,
            "source" => source,
            "installed" => installed,
            "selected" => selected.contains_key(name),
            "default" => defaults.iter().any(|default| default == name),
        });
    };
    for plugin in installed.values() {
        add(plugin, true);
    }
    for plugin in util::list(catalog, "builtins") {
        add(plugin, true);
    }
    for plugin in util::list(
        obj(catalog.get("catalog").unwrap_or(&Value::Null)),
        "entries",
    ) {
        add(plugin, false);
    }
    plugins.sort_by(|a, b| text(a, "name").cmp(&text(b, "name")));
    Ok(plugins)
}

/// What `denial-plugins list --json` prints.
pub fn list_report(store: &Store, catalog: &Map) -> Result<Map> {
    let community = obj(catalog.get("catalog").unwrap_or(&Value::Null));
    let selection = store.selection()?;
    let roots = obj(&selection["roots"]);
    let defaults = default_names(catalog);
    let default_selection = roots.len() == defaults.len()
        && roots
            .iter()
            .all(|(name, root)| defaults.contains(name) && root["source"]["kind"] == "builtin");
    Ok(object! {
        "contract" => CONTRACT,
        "plugins" => plugin_list(store, catalog)?.into_iter().map(Value::Object).collect::<Vec<_>>(),
        // Whether the selection is exactly the plugins Denial comes with.
        "default_selection" => default_selection,
        // The community list: whether one is set, and when it was read.
        "community" => object! {
            "configured" => catalog.get("configured") == Some(&Value::Bool(true)),
            "updated" => community.get("updated").cloned().unwrap_or(Value::Null),
        },
        // The community list could not be loaded; built-ins still work.
        "errors" => util::list(community, "errors").iter().map(|error| match error {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }).collect::<Vec<_>>(),
    })
}

/// Requests only: what a request runs inside itself is its own business.
fn requests(jobs: &[Map]) -> Vec<&Map> {
    jobs.iter()
        .filter(|job| {
            obj(job.get("arguments").unwrap_or(&Value::Null))
                .get("request")
                .is_none_or(Value::is_null)
        })
        .collect()
}

/// What a request was asked: `apply` also when a terminal ran it.
fn operation(job: &Map) -> String {
    text(obj(job.get("arguments").unwrap_or(&Value::Null)), "command")
        .or_else(|| text(job, "operation"))
        .unwrap_or("")
        .to_owned()
}

/// When a request ended, or last said something.
fn ended(job: &Map) -> String {
    text(job, "updated")
        .or_else(|| text(job, "created"))
        .unwrap_or("")
        .to_owned()
}

fn phase(job: &Map) -> &str {
    text(job, "phase").unwrap_or("")
}

fn detail(job: &Map) -> String {
    match job.get("error") {
        Some(Value::String(error)) => error.trim().to_owned(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string().trim().to_owned(),
    }
}

/// What `denial-plugins history --json` prints, the newest first. Looking
/// into a folder or a link is no request worth remembering.
pub fn history_report(jobs: &[Map]) -> Map {
    let mut shown: Vec<&Map> = requests(jobs)
        .into_iter()
        .filter(|job| {
            text(job, "operation") != Some("inspect")
                && (text(job, "operation") != Some("plan")
                    || obj(job.get("arguments").unwrap_or(&Value::Null))
                        .get("command")
                        .is_some_and(|c| !c.is_null()))
        })
        .collect();
    shown.sort_by_key(|job| std::cmp::Reverse(ended(job)));
    let entries: Vec<Value> = shown
        .into_iter()
        .map(|job| {
            let state = match phase(job) {
                "queued" => "waiting",
                "running" => "working",
                "succeeded" => "done",
                "interrupted" => "stopped",
                _ => "failed",
            };
            let mut entry = object! {
                "job" => job.get("id").cloned().unwrap_or(Value::Null),
                "operation" => operation(job),
                "state" => state,
                "when" => ended(job),
            };
            if state == "failed" || state == "stopped" {
                let detail = detail(job);
                let code = if state == "stopped" {
                    "stopped".to_owned()
                } else {
                    failure_code(&detail).to_owned()
                };
                entry.extend(object! {
                    "code" => code.clone(),
                    "text" => failure_text(&code),
                    "detail" => detail,
                });
            }
            Value::Object(entry)
        })
        .collect();
    object! {"contract" => CONTRACT, "entries" => entries}
}

pub fn describe_history(history: &Map) -> String {
    let entries = util::list(history, "entries");
    if entries.is_empty() {
        return "Nothing was asked yet.".into();
    }
    entries
        .iter()
        .map(|entry| {
            let detail = entry["detail"].as_str().unwrap_or("");
            format!(
                "{:<28}{:<16}{}{}",
                entry["when"].as_str().unwrap_or(""),
                entry["operation"].as_str().unwrap_or(""),
                entry["state"].as_str().unwrap_or(""),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn describe_plugins(plugins: &[Value]) -> String {
    if plugins.is_empty() {
        return "No plugins are known yet.".into();
    }
    let width = plugins
        .iter()
        .map(|plugin| plugin["name"].as_str().unwrap_or("").len())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<String> = plugins
        .iter()
        .map(|plugin| {
            format!(
                "{} {:<width$}  {}",
                if plugin["selected"] == true {
                    "on "
                } else {
                    "   "
                },
                plugin["name"].as_str().unwrap_or(""),
                plugin["from"].as_str().unwrap_or("")
            )
        })
        .collect();
    lines.push(String::new());
    lines.push("on = selected. denial-plugins add NAME, denial-plugins remove NAME, then denial-plugins apply.".into());
    lines.join("\n")
}

fn providers_found(error: &str) -> Vec<u64> {
    error
        .match_indices("found ")
        .filter_map(|(at, _)| {
            let rest = &error[at + "found ".len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let after = &rest[digits.len()..];
            (!digits.is_empty() && after.starts_with(" providers"))
                .then(|| digits.parse().ok())
                .flatten()
        })
        .collect()
}

/// Why an operation failed, from the message it ended with: the manager's
/// own messages and deniald's.
pub fn failure_code(error: &str) -> &'static str {
    if error.contains("plugin bundle source does not match installed Denial") {
        return "other_denial";
    }
    if error.contains("compatibility declarations") {
        return "declaration";
    }
    // What the selection check says before anything is built.
    if error.contains("Disable") && error.contains("before applying.") {
        return "conflict";
    }
    if error.contains("Enable a compatible plugin before applying.") {
        return "missing";
    }
    if error.contains("#ShellPanel; found") && !error.contains("found 0 providers") {
        return "two_panels";
    }
    let found = providers_found(error);
    if found.iter().any(|count| *count >= 2) {
        return "conflict";
    }
    if error.contains("found 0 providers")
        || error.contains("has no provider")
        || error.contains("no provider is enabled")
    {
        return "missing";
    }
    if error.contains("Log out and back") {
        return "log_in_again";
    }
    if error.contains("did not start") {
        return "did_not_start";
    }
    if error.contains("SocketException")
        || error.contains("Could not resolve host")
        || error.contains("Connection timed out")
    {
        return "download";
    }
    "failed"
}

pub fn failure_text(code: &str) -> &'static str {
    match code {
        "other_denial" => "Your plugins were built for another Denial. Apply builds them again.",
        "declaration" => "A plugin describes itself wrongly. The details name it.",
        "two_panels" => "More than one desktop panel is selected. Keep one, then apply again.",
        "conflict" => "Several plugins provide the same feature. Keep one, then apply again.",
        "missing" => {
            "A feature the desktop needs is missing. Turn on a plugin that provides it, then apply again."
        }
        "log_in_again" => "Log out and back into Denial once, then apply again.",
        "did_not_start" => {
            "Your plugins were built but did not start. Denial kept the desktop it was running."
        }
        "download" => "A download failed. Check the connection and apply again.",
        "stopped" => "The last change was stopped before it finished.",
        _ => "The last change failed. The details say why.",
    }
}

/// What `denial-plugins status` says: what runs, why, and what to do next,
/// from the status `state` the command gathered.
pub fn summarize(store: &Store, state: &Map) -> Result<Map> {
    let native = obj(state.get("native").unwrap_or(&Value::Null));
    let selection = obj(state.get("selection").unwrap_or(&Value::Null));
    let active = obj(state.get("active").unwrap_or(&Value::Null));
    let selected = sorted_keys(selection.get("roots"));
    let mut why: Vec<Value> = Vec::new();
    let mut next: Option<Value> = None;
    let apply = || Value::Object(object! {"code" => "apply", "text" => "denial-plugins apply"});

    let reachable = native.get("available") != Some(&Value::Bool(false));
    let capable = reachable
        && util::strings(util::list(native, "capabilities"))
            .iter()
            .any(|capability| capability == BUNDLE_CAPABILITY);
    let custom = reachable
        && native.get("plugin_bundle").is_some_and(Value::is_string)
        && text(native, "active_mode") == Some("custom_optimized");

    let desktop = if !reachable {
        let error = match native.get("error") {
            Some(Value::String(error)) => error.trim().to_owned(),
            Some(other) => other.to_string(),
            None => "null".into(),
        };
        why.push(line("session_error", &error, Map::new()));
        next = Some(line(
            "open_in_session",
            "Run this from a terminal inside your Denial session.",
            Map::new(),
        ));
        line(
            "unreachable",
            "Denial does not answer in this session.",
            Map::new(),
        )
    } else if custom {
        let desktop = match text(active, "id") {
            None => {
                let bundle = native.get("plugin_bundle").cloned().unwrap_or(Value::Null);
                line(
                    "other_plugins",
                    &format!(
                        "Plugins built outside this plugin manager are running: {}",
                        bundle.as_str().unwrap_or("")
                    ),
                    object! {"bundle" => bundle},
                )
            }
            Some(id) => {
                let plan = store.read(&format!("candidates/{id}/plan.json"))?;
                let running = sorted_keys(plan.get("roots"));
                if !same_composition(selection, &plan) {
                    why.push(line(
                        "selection_changed",
                        &format!("Your selection changed since then: {}", selected.join(", ")),
                        Map::new(),
                    ));
                    next = Some(apply());
                }
                line(
                    "plugins",
                    &format!("Your plugins are running: {}", running.join(", ")),
                    object! {"plugins" => running},
                )
            }
        };
        if native.get("plugin_engine_differs") == Some(&Value::Bool(true)) {
            why.push(line(
                "engine_differs",
                "They were built for another engine than this session runs, and loaded anyway.",
                Map::new(),
            ));
        }
        if native.get("plugin_healthy") != Some(&Value::Bool(true)) {
            why.push(line("starting", "They are still starting.", Map::new()));
        }
        desktop
    } else {
        line("standard", "The standard desktop is running.", Map::new())
    };

    let rebuild = obj(native.get("plugin_rebuild").unwrap_or(&Value::Null));
    let waiting = rebuild.get("bundle").is_some_and(Value::is_string);
    if reachable && waiting {
        why.push(if text(rebuild, "reason") == Some("engine") {
            line(
                "rebuild_engine",
                "Your plugins did not start on this session's engine. They have to be built again.",
                Map::new(),
            )
        } else {
            let mut facts = Map::new();
            if let Some(version) = text(rebuild, "version").filter(|version| is_release(version)) {
                facts.insert("version".into(), version.into());
            }
            line(
                "rebuild_update",
                "Denial changed since your plugins were built. They have to be built again.",
                facts,
            )
        });
        next = Some(apply());
    }
    if reachable
        && !custom
        && let Some(error) = text(native, "error")
            .map(str::trim)
            .filter(|error| !error.is_empty())
    {
        why.push(line(
            "denial_said",
            &format!("Denial said: {error}"),
            object! {"message" => error},
        ));
    }

    // What stops the saved selection from being built at all.
    let issues: Vec<Value> = util::list(
        obj(state.get("preflight").unwrap_or(&Value::Null)),
        "issues",
    )
    .iter()
    .filter(|issue| issue.is_object())
    .map(|issue| {
        let mut entry = object! {
            "code" => issue.get("code").cloned().unwrap_or(Value::Null),
            "text" => issue.get("message").cloned().unwrap_or(Value::Null),
            "plugins" => issue.get("plugins").cloned().unwrap_or_else(|| Value::Array(vec![])),
        };
        if let Some(feature) = issue
            .get("requirement")
            .and_then(|requirement| requirement.get("label"))
            .filter(|label| !label.is_null())
        {
            entry.insert("feature".into(), feature.clone());
        }
        Value::Object(entry)
    })
    .collect();

    let jobs: Vec<Map> = util::list(state, "jobs")
        .iter()
        .filter_map(|job| job.as_object().cloned())
        .collect();
    let all = requests(&jobs);
    let working = all
        .iter()
        .find(|job| ["queued", "running"].contains(&phase(job)))
        .copied();
    // The request that ended last: one refused because another ran ended
    // before that other one did.
    let mut finished: Option<&Map> = None;
    for job in &all {
        if ["succeeded", "failed", "interrupted"].contains(&phase(job))
            && !["inspect", "refresh-catalog"].contains(&text(job, "operation").unwrap_or(""))
            && finished.is_none_or(|latest| ended(job) > ended(latest))
        {
            finished = Some(job);
        }
    }

    let work = working.map(|working| {
        let progress = match working.get("progress") {
            Some(Value::Object(progress)) => Some(progress.clone()),
            _ => text(working, "message").and_then(|message| job_progress(&operation(working), message)),
        };
        let label = progress
            .as_ref()
            .and_then(|progress| progress.get("label").cloned())
            .unwrap_or_else(|| {
                Value::from(if phase(working) == "queued" {
                    "Waiting to start"
                } else if text(working, "operation") == Some("refresh-catalog") {
                    "Reading the community list"
                } else {
                    "Working on your changes"
                })
            });
        let step = progress
            .as_ref()
            .and_then(|progress| progress.get("completed").and_then(Value::as_i64))
            .map_or(Value::Null, |completed| Value::from(completed + 1));
        let started = progress
            .as_ref()
            .and_then(|progress| progress.get("started").cloned())
            .filter(|started| !started.is_null())
            .or_else(|| working.get("updated").cloned())
            .or_else(|| working.get("created").cloned())
            .unwrap_or(Value::Null);
        object! {
            "job" => working.get("id").cloned().unwrap_or(Value::Null),
            "operation" => operation(working),
            // A label of progress.rs: a protocol value, never reworded.
            "label" => label,
            "step" => step,
            "steps" => progress.as_ref().and_then(|progress| progress.get("total").cloned()).unwrap_or(Value::Null),
            // When this step began; a clock counts from it.
            "started" => started,
        }
    });

    let failure = match (working, finished) {
        (None, Some(finished)) if phase(finished) != "succeeded" => {
            let detail = detail(finished);
            let code = if phase(finished) == "interrupted" {
                "stopped"
            } else {
                failure_code(&detail)
            };
            Some(object! {
                "job" => finished.get("id").cloned().unwrap_or(Value::Null),
                "operation" => operation(finished),
                "code" => code,
                "text" => failure_text(code),
                "detail" => detail,
            })
        }
        _ => None,
    };

    // What the last build of this very selection took in besides the
    // selected plugins, and through which of them.
    let plan = obj(state.get("plan").unwrap_or(&Value::Null));
    let discovery = obj(plan.get("discovery").unwrap_or(&Value::Null));
    let roots = obj(selection.get("roots").unwrap_or(&Value::Null));
    let revision = plan
        .get("selectionRevision")
        .filter(|revision| !revision.is_null());
    let included: Vec<Value> = if revision.is_some() && revision == selection.get("revision") {
        util::strings(util::list(discovery, "plugins"))
            .into_iter()
            .filter(|name| !roots.contains_key(name))
            .map(|name| {
                let chain = obj(discovery.get("dependencyChains").unwrap_or(&Value::Null))
                    .get(&name)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let through = if chain.len() > 2 {
                    chain[1..chain.len() - 1].to_vec()
                } else {
                    Vec::new()
                };
                Value::Object(object! {
                    "name" => name.clone(),
                    "title" => plugin_title(&name),
                    // From a selected plugin down to the one that needs it.
                    "through" => through,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // Whether this installation can build plugins at all.
    let dart = obj(state.get("dart").unwrap_or(&Value::Null));
    let problem = text(state, "problem");
    let dart_missing = dart.get("available") == Some(&Value::Bool(false));
    let setup = if let Some(problem) = problem {
        if dart_missing
            && dart.get("version").is_some_and(Value::is_string)
            && dart.get("constraint").is_some_and(Value::is_string)
        {
            line(
                "incompatible_dart",
                problem,
                object! {"installed" => dart["version"].clone(), "required" => dart["constraint"].clone()},
            )
        } else if dart_missing && dart.get("expectedVersion").is_some_and(Value::is_string) {
            line(
                "missing_dart",
                problem,
                object! {"expected" => dart["expectedVersion"].clone()},
            )
        } else {
            line("tools", problem, Map::new())
        }
    } else if state.get("ready") == Some(&Value::Bool(true)) {
        line("ready", "Plugin support is ready.", Map::new())
    } else if working.is_some_and(|working| {
        ["initialize", "prepare"].contains(&text(working, "operation").unwrap_or(""))
    }) {
        line("preparing", "Plugin support is being prepared.", Map::new())
    } else {
        line(
            "needed",
            "Plugin support is prepared the first time you apply.",
            Map::new(),
        )
    };

    if reachable && !capable {
        why.push(line(
            "session_old",
            "This session's Denial is older than this plugin manager and cannot load what it builds.",
            Map::new(),
        ));
        next = Some(line(
            "log_in_again",
            "Log out and back into Denial once.",
            Map::new(),
        ));
    }
    let next_code = |next: &Option<Value>| {
        next.as_ref()
            .and_then(|next| next["code"].as_str().map(str::to_owned))
    };
    if !issues.is_empty() && next_code(&next).as_deref() != Some("log_in_again") && reachable {
        next = Some(line(
            "fix_selection",
            "Change your selection with denial-plugins add or remove, then denial-plugins apply.",
            Map::new(),
        ));
    }
    if problem.is_some() {
        next = None;
    }
    if work.is_some() {
        next = Some(line(
            "wait",
            "Wait for it, then denial-plugins status.",
            Map::new(),
        ));
    } else if capable && problem.is_none() && issues.is_empty() {
        if failure.as_ref().is_some_and(|failure| {
            ["plan", "apply", "update", "rebuild"]
                .contains(&text(failure, "operation").unwrap_or(""))
        }) && next.is_none()
        {
            next = Some(apply());
        }
        if !custom && !selected.is_empty() && !waiting && next.is_none() {
            next = Some(apply());
        }
    }

    let idle = work.is_none();
    let switch_now = work.as_ref().is_some_and(|work| {
        text(work, "operation") == Some("rebuild")
            && text(work, "label") == Some("Switching when you pause")
    });
    Ok(object! {
        "contract" => CONTRACT,
        "desktop" => desktop,
        "selected" => selected,
        "why" => why,
        "issues" => issues.clone(),
        "included" => included,
        "setup" => setup,
        "work" => work.map_or(Value::Null, Value::Object),
        "failure" => failure.map_or(Value::Null, Value::Object),
        "next" => next.unwrap_or(Value::Null),
        // What a program may offer now. Apply stays allowed where it can
        // work: in a development checkout it is also how edits are tried.
        "can" => object! {
            "change" => idle && problem.is_none(),
            "apply" => idle && capable && problem.is_none() && issues.is_empty(),
            "undo" => idle && reachable && native.get("plugin_previous").is_some_and(Value::is_string),
            "off" => idle && custom,
            "switch_now" => switch_now,
        },
    })
}

pub fn describe_status(summary: &Map) -> String {
    let selected = util::strings(util::list(summary, "selected"));
    let setup = obj(summary.get("setup").unwrap_or(&Value::Null));
    let mut lines = vec![
        obj(summary.get("desktop").unwrap_or(&Value::Null))
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        format!(
            "Selected: {}",
            if selected.is_empty() {
                "nothing".to_owned()
            } else {
                selected.join(", ")
            }
        ),
    ];
    for why in util::list(summary, "why") {
        lines.push(format!("  {}", why["text"].as_str().unwrap_or("")));
    }
    for issue in util::list(summary, "issues") {
        lines.push(format!("  {}", issue["text"].as_str().unwrap_or("")));
    }
    if ["tools", "missing_dart", "incompatible_dart"].contains(&text(setup, "code").unwrap_or("")) {
        lines.push(format!("  {}", text(setup, "text").unwrap_or("")));
    }
    if let Some(work) = summary.get("work").and_then(Value::as_object) {
        let step = match work.get("step").and_then(Value::as_i64) {
            Some(step) => format!(
                " (step {step} of {})",
                work.get("steps")
                    .map(|steps| steps.to_string())
                    .unwrap_or_default()
            ),
            None => String::new(),
        };
        lines.push(format!(
            "  Working: {}{step}",
            text(work, "label").unwrap_or("")
        ));
    }
    if let Some(failure) = summary.get("failure").and_then(Value::as_object) {
        let detail = text(failure, "detail").unwrap_or("");
        lines.push(format!(
            "  The last change failed: {}",
            if detail.is_empty() {
                text(failure, "text").unwrap_or("")
            } else {
                detail
            }
        ));
    }
    if let Some(next) = summary.get("next").and_then(Value::as_object) {
        lines.push(format!("Next: {}", text(next, "text").unwrap_or("")));
    }
    lines.join("\n")
}
