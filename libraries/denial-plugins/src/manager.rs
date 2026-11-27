//! The commands of the plugin manager, the way `denial-plugins` and Settings
//! ask for them, and requests: commands that go on by themselves while the
//! program that asked carries on, recorded as jobs that `status` reports.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::activation::Activation;
use crate::available::Available;
use crate::builder::{self, BuildRequest};
use crate::discovery::Runner;
use crate::graph::Graph;
use crate::installation::Installation;
use crate::notifications::{self, DesktopNotifications};
use crate::object;
use crate::preflight;
use crate::progress::job_progress;
use crate::rebuild::{self, Target};
use crate::report;
use crate::resume::{Resume, Session};
use crate::source::{Kind as SourceKind, Repositories, Resolved, Source};
use crate::store::{Store, process_start, validate_id};
use crate::tools::Tools;
use crate::util::{self, Error, Map, Result, absolute, display, fail, obj, text};
use crate::workspace::{PlanRequest, Planner};

/// What `denial-plugins` prints without arguments.
pub const USAGE: &str = "\
Denial plugins

  denial-plugins status         What runs now, why, and what to do next
  denial-plugins list           Plugins you can use, and which are on
  denial-plugins add PLUGIN     Turn a plugin on: a name from list, a folder, or a Git URL
  denial-plugins remove PLUGIN  Turn a plugin off
  denial-plugins defaults       Select the plugins Denial comes with, and only them
  denial-plugins apply          Build your selection and switch the desktop to it
  denial-plugins undo           Go back to the plugins that ran before
  denial-plugins off            Use the standard desktop; your selection is kept
  denial-plugins history        What was asked lately, and how it ended

add and remove only change the selection. Nothing changes on screen until apply.
";

/// What `--help` adds about the options.
pub const OPTIONS: &str = "\
    --ref       add: Git branch, tag, or commit
    --path      add: package path within the repository
                (defaults to \".\")
    --update    apply: also move Git plugins to their newest revision
    --json      Print JSON instead of text
    --help
";

/// The operations a request may run.
const REQUESTS: [&str; 16] = [
    "prepare",
    "initialize",
    "defaults",
    "refresh-catalog",
    "inspect",
    "add",
    "enable",
    "remove",
    "plan",
    "apply",
    "restore",
    "revert",
    "undo",
    "off",
    "update",
    "rebuild",
];

/// A command line, as `denial-plugins` takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub rest: Vec<String>,
    pub state: Option<PathBuf>,
    pub reference: Option<String>,
    pub path: Option<String>,
    pub local: bool,
    pub builtin: bool,
    pub update: bool,
    pub now: bool,
    pub json: bool,
    pub brief: bool,
    pub help: bool,
}

impl Options {
    pub fn parse<S: AsRef<str>>(arguments: &[S]) -> Result<Self> {
        let mut options = Options::default();
        let mut index = 0;
        while index < arguments.len() {
            let argument = arguments[index].as_ref();
            index += 1;
            if argument == "--" {
                options
                    .rest
                    .extend(arguments[index..].iter().map(|a| a.as_ref().to_owned()));
                break;
            }
            let Some(name) = argument.strip_prefix("--") else {
                options.rest.push(argument.to_owned());
                continue;
            };
            let (name, inline) = match name.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (name, None),
            };
            let mut value = || -> Result<String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                let value = arguments
                    .get(index)
                    .map(|value| value.as_ref().to_owned())
                    .ok_or_else(|| Error::format(format!("Missing argument for \"{name}\".")))?;
                index += 1;
                Ok(value)
            };
            match name {
                "state" => options.state = Some(PathBuf::from(value()?)),
                "ref" => options.reference = Some(value()?),
                "path" => options.path = Some(value()?),
                flag => {
                    let (flag, on) = match flag.strip_prefix("no-") {
                        Some(flag) => (flag, false),
                        None => (flag, true),
                    };
                    let target = match flag {
                        "local" => &mut options.local,
                        "builtin" => &mut options.builtin,
                        "update" => &mut options.update,
                        "now" => &mut options.now,
                        "json" => &mut options.json,
                        "brief" => &mut options.brief,
                        "help" => &mut options.help,
                        _ => {
                            return Err(Error::format(format!(
                                "Could not find an option named \"--{name}\"."
                            )));
                        }
                    };
                    if inline.is_some() {
                        return Err(Error::format(format!(
                            "Flag option \"{flag}\" should not be given a value."
                        )));
                    }
                    *target = on;
                }
            }
        }
        Ok(options)
    }

    fn package_path(&self) -> &str {
        self.path.as_deref().unwrap_or(".")
    }
}

/// What a command answered: JSON, and for people a sentence, if it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub result: Value,
    pub text: Option<String>,
}

impl Outcome {
    fn json(result: impl Into<Value>) -> Self {
        Self {
            result: result.into(),
            text: None,
        }
    }

    fn said(result: impl Into<Value>, text: impl Into<String>) -> Self {
        Self {
            result: result.into(),
            text: Some(text.into()),
        }
    }
}

/// Requests running in this process.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Whether a request of this process still runs: a program that asked for
/// one should not end before it does.
pub fn working() -> bool {
    RUNNING.load(Ordering::SeqCst) > 0
}

/// Where a command's progress goes: to a terminal, or into the request it
/// runs for.
pub struct Context {
    request: Option<(Store, String, String)>,
    print: bool,
    last: Mutex<Option<Map>>,
}

impl Context {
    /// A command run from a terminal: progress is printed.
    pub fn terminal() -> Self {
        Self {
            request: None,
            print: true,
            last: Mutex::new(None),
        }
    }

    /// A short command a program runs: its progress goes nowhere.
    pub fn silent() -> Self {
        Self {
            request: None,
            print: false,
            last: Mutex::new(None),
        }
    }

    fn request(store: &Store, id: &str, operation: &str) -> Self {
        Self {
            request: Some((store.clone(), id.to_owned(), operation.to_owned())),
            print: false,
            last: Mutex::new(None),
        }
    }

    fn id(&self) -> Option<&str> {
        self.request.as_ref().map(|(_, id, _)| id.as_str())
    }

    /// Reports one line of progress.
    pub fn say(&self, line: &str) {
        let Some((store, id, operation)) = &self.request else {
            if self.print {
                eprintln!("{line}");
            }
            return;
        };
        if !line.trim().is_empty() {
            let mut update = object! {"message" => line};
            if let Some(mut progress) = job_progress(operation, line) {
                let mut last = self
                    .last
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                let same = last.as_ref().is_some_and(|last| {
                    last.get("label") == progress.get("label")
                        && last.get("completed") == progress.get("completed")
                });
                let started = match (&*last, same) {
                    (Some(last), true) => last.get("started").cloned().unwrap_or(Value::Null),
                    _ => Value::from(util::now()),
                };
                progress.insert("started".into(), started);
                *last = Some(progress.clone());
                update.insert("progress".into(), Value::Object(progress));
            }
            let _ = store.update_job(id, update);
        }
        append(
            &store.file(&format!("jobs/{id}.progress.log")),
            &format!("{line}\n"),
        );
    }
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = file.write_all(text.as_bytes());
    }
}

/// Decrements the count of running requests however a request ends.
struct Running;

impl Running {
    fn start() -> Self {
        RUNNING.fetch_add(1, Ordering::SeqCst);
        Running
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The plugin manager of one installation, over one state folder.
#[derive(Clone, Debug)]
pub struct Manager {
    pub store: Store,
    pub installation: Installation,
}

impl Manager {
    pub fn new(store: Store, installation: Installation) -> Self {
        if !installation.path.is_empty() {
            util::search_first(&installation.path);
        }
        Self {
            store,
            installation,
        }
    }

    /// The manager of the Denial that runs, over the user's state: what
    /// Settings uses. None without plugin support.
    pub fn of_session() -> Option<Result<Self>> {
        Installation::command()?;
        Some(
            Installation::locate()
                .map(|installation| Self::new(Store::new(Store::default_root()), installation)),
        )
    }

    fn repositories(&self) -> Repositories {
        Repositories::new(self.store.file("repositories"))
    }

    fn tools(&self) -> Tools {
        Tools::of(&self.store, &self.installation)
    }

    fn activation(&self) -> Activation {
        Activation {
            store: self.store.clone(),
            denialctl: self.installation.denialctl.clone(),
        }
    }

    fn initialized(&self, tools: &Tools) -> bool {
        tools.ready()
            && self
                .store
                .read("initialization.json")
                .is_ok_and(|record| record.get("complete") == Some(&Value::Bool(true)))
    }

    // Requests.

    /// Records a request without starting it.
    pub fn queue<S: AsRef<str>>(&self, argv: &[S]) -> Result<String> {
        let argv: Vec<String> = argv
            .iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        if argv
            .first()
            .is_none_or(|operation| !REQUESTS.contains(&operation.as_str()))
        {
            return fail("Unsupported background operation");
        }
        self.store
            .create_job(&argv[0], object! {"argv" => argv.clone()})
    }

    /// Starts a request on its own thread of this process; `status` reports
    /// it. Returns its job.
    pub fn submit<S: AsRef<str>>(&self, argv: &[S]) -> Result<String> {
        let id = self.queue(argv)?;
        self.start(&id)?;
        Ok(id)
    }

    /// Runs request `id`, queued before, on its own thread.
    pub fn start(&self, id: &str) -> Result<()> {
        let manager = self.clone();
        let job = id.to_owned();
        let running = Running::start();
        let started = std::thread::Builder::new()
            .name("denial-plugins request".into())
            .spawn(move || {
                let _running = running;
                manager.run_request(&job);
            });
        if let Err(error) = started {
            self.store.update_job(
                id,
                object! {"phase" => "failed", "error" => error.to_string()},
            )?;
            return Err(error.into());
        }
        Ok(())
    }

    /// Runs queued request `id` here and records how it ended.
    pub fn run_request(&self, id: &str) {
        if let Err(error) = self.try_run_request(id) {
            let _ = self
                .store
                .update_job(id, object! {"phase" => "failed", "error" => error.message});
        }
    }

    fn try_run_request(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        let job = self.store.read(&format!("jobs/{id}.json"))?;
        if text(&job, "phase") != Some("queued") {
            return fail("Job has already started");
        }
        let pid = std::process::id();
        self.store.update_job(
            id,
            object! {"phase" => "running", "pid" => pid, "processStart" => process_start(pid)},
        )?;
        let argv = util::strings(util::list(
            obj(job.get("arguments").unwrap_or(&Value::Null)),
            "argv",
        ));
        let context = Context::request(&self.store, id, argv.first().map_or("", String::as_str));
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            Options::parse(&argv).and_then(|options| self.execute(&options, &context))
        }))
        .unwrap_or_else(|panic| {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    panic
                        .downcast_ref::<&str>()
                        .map(|reason| (*reason).to_owned())
                })
                .unwrap_or_else(|| "unknown".into());
            fail(format!("The request stopped unexpectedly: {reason}"))
        });
        let log = self.store.file(&format!("jobs/{id}.log"));
        match outcome {
            Ok(outcome) => {
                let _ = std::fs::write(&log, format!("{}\n", util::pretty(&outcome.result)));
                let mut update = object! {"phase" => "succeeded", "result" => outcome.result};
                if let Some(Value::Object(mut progress)) = self
                    .store
                    .read(&format!("jobs/{id}.json"))?
                    .remove("progress")
                {
                    let total = progress.get("total").cloned().unwrap_or(Value::Null);
                    progress.insert("completed".into(), total);
                    progress.insert("label".into(), "Completed".into());
                    update.insert("progress".into(), Value::Object(progress));
                }
                self.store.update_job(id, update)
            }
            Err(error) => {
                let _ = std::fs::write(&log, format!("{error}\n"));
                context.say(&format!("denial-plugins: {error}"));
                self.store
                    .update_job(id, object! {"phase" => "failed", "error" => error.message})
            }
        }
    }

    /// Prepares plugin support in the background where it is not yet:
    /// `{ready: true}`, or the request that prepares it.
    pub fn bootstrap(&self) -> Result<Map> {
        let tools = self.tools();
        let pending = || -> Result<Option<Map>> {
            Ok(self
                .store
                .jobs()?
                .into_iter()
                .find(|job| {
                    text(job, "operation") == Some("initialize")
                        && ["queued", "running"].contains(&text(job, "phase").unwrap_or(""))
                })
                .map(|job| object! {"job" => job.get("id").cloned().unwrap_or(Value::Null)}))
        };
        if self.initialized(&tools) {
            return Ok(object! {"ready" => true});
        }
        if let Some(pending) = pending()? {
            return Ok(pending);
        }
        let queued = self.store.exclusive(|| {
            if self.initialized(&tools) {
                return Ok(Err(object! {"ready" => true}));
            }
            if let Some(pending) = pending()? {
                return Ok(Err(pending));
            }
            self.queue(&["initialize"]).map(Ok)
        })?;
        match queued {
            // Started once the lock is free again, which it takes.
            Ok(id) => {
                self.start(&id)?;
                Ok(object! {"job" => id})
            }
            Err(answer) => Ok(answer),
        }
    }

    // Reports.

    /// What `status` says: what runs, why, and what can be done, under
    /// `summary`. `brief` leaves out what only a person debugging reads.
    pub fn status(&self, brief: bool) -> Result<Map> {
        let store = &self.store;
        let tools = self.tools();
        let activation = self.activation();
        let native = activation.status();
        let selection = store.selection()?;
        let last = store.read("last-plan.json")?;
        let installed = store.read_or(
            "installed.json",
            object! {"plugins" => selection.get("roots").cloned().unwrap_or(Value::Null)},
        )?;
        let running = activation.running_candidate(&native);
        let (plan, build) = match text(&last, "id") {
            Some(id) => {
                validate_id(id)?;
                (
                    store.read(&format!("candidates/{id}/plan.json"))?,
                    store.read(&format!("candidates/{id}/build.json"))?,
                )
            }
            None => (Map::new(), Map::new()),
        };
        let mut known = obj(installed.get("plugins").unwrap_or(&Value::Null)).clone();
        for (name, root) in obj(selection.get("roots").unwrap_or(&Value::Null)) {
            known.insert(name.clone(), root.clone());
        }
        let jobs = store.jobs()?;
        let mut state = object! {
            "plan" => plan,
            "build" => build,
            "native" => native.clone(),
            "configuration" => tools.configuration(),
            "buildKitAvailable" => tools.available(),
            "problem" => tools.problem(),
            "dart" => tools.dart_status(),
            "ready" => self.initialized(&tools),
            "selection" => selection.clone(),
            "declarations" => preflight::describe(&known),
            "preflight" => preflight::check(&selection),
            "installed" => installed,
            "lastPlan" => last,
            // What deniald runs, when this manager built it. deniald is the
            // only record; nothing is kept here.
            "active" => object! {
                "id" => running.clone(),
                "bundle" => if running.is_some() { native.get("plugin_bundle").cloned().unwrap_or(Value::Null) } else { Value::Null },
                "previous" => activation.candidate_of(native.get("plugin_previous")),
            },
            "jobs" => jobs.iter().cloned().map(Value::Object).collect::<Vec<_>>(),
        };
        let summary = report::summarize(store, &state)?;
        state.insert("summary".into(), Value::Object(summary));
        if brief {
            let keep = |record: &Value, keys: &[&str]| -> Value {
                Value::Object(
                    obj(record)
                        .iter()
                        .filter(|(key, _)| keys.contains(&key.as_str()))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                )
            };
            let plan = keep(
                &state["plan"],
                &["id", "roots", "discovery", "status", "selectionRevision"],
            );
            // Settings needs the activation identity to tell a pending
            // selection from a built composition already on the desktop.
            let build = keep(&state["build"], &["status", "bundle", "reusedFrom"]);
            let jobs: Vec<Value> = jobs
                .iter()
                .filter(|job| {
                    obj(job.get("arguments").unwrap_or(&Value::Null)).contains_key("argv")
                        || ["running", "queued"].contains(&text(job, "phase").unwrap_or(""))
                })
                .map(|job| {
                    let mut job = job.clone();
                    job.shift_remove("result");
                    Value::Object(job)
                })
                .collect();
            state.insert("plan".into(), plan);
            state.insert("build".into(), build);
            state.insert("jobs".into(), Value::Array(jobs));
        }
        Ok(state)
    }

    /// The summary of `status`, the part a program shows.
    pub fn summary(&self) -> Result<Map> {
        Ok(obj(self.status(true)?.get("summary").unwrap_or(&Value::Null)).clone())
    }

    /// Every plugin this installation knows (`denial-plugins list`).
    pub fn list(&self) -> Result<Map> {
        let configuration = self.tools().configuration();
        let repositories = self.repositories();
        let catalog = Available {
            store: &self.store,
            repositories: &repositories,
        }
        .list(&configuration)?;
        report::list_report(&self.store, &catalog)
    }

    /// The requests the manager remembers (`denial-plugins history`).
    pub fn history(&self) -> Result<Map> {
        Ok(report::history_report(&self.store.jobs()?))
    }

    /// A request's record and what it printed while it ran.
    pub fn job_details(&self, id: &str) -> Result<Map> {
        validate_id(id)?;
        let log = std::fs::read_to_string(self.store.file(&format!("jobs/{id}.progress.log")))
            .unwrap_or_default();
        Ok(object! {"job" => self.store.read(&format!("jobs/{id}.json"))?, "log" => log})
    }

    /// Asks a rebuild that waits for a pause to switch the desktop now. Not
    /// a change: the waiting rebuild owns the switch and its checks.
    pub fn switch_now(&self) -> Result<()> {
        std::fs::create_dir_all(&self.store.root)
            .map_err(|error| Error::file(&self.store.root, &error))?;
        std::fs::write(rebuild::switch_now_request(&self.store), "")?;
        Ok(())
    }

    // The command line.

    /// Runs one command and returns what it answered.
    pub fn execute(&self, options: &Options, context: &Context) -> Result<Outcome> {
        let rest: Vec<&str> = options.rest.iter().map(String::as_str).collect();
        let store = &self.store;
        match rest.as_slice() {
            ["bootstrap"] => Ok(Outcome::json(self.bootstrap()?)),
            ["job-details", id] => Ok(Outcome::json(self.job_details(id)?)),
            ["catalog"] => {
                let repositories = self.repositories();
                let configuration = self.tools().configuration();
                Ok(Outcome::json(
                    Available {
                        store,
                        repositories: &repositories,
                    }
                    .list(&configuration)?,
                ))
            }
            ["list"] => {
                let report = self.list()?;
                let text = report::describe_plugins(util::list(&report, "plugins"));
                Ok(Outcome::said(report, text))
            }
            ["history"] => {
                let history = self.history()?;
                let text = report::describe_history(&history);
                Ok(Outcome::said(history, text))
            }
            ["status"] => {
                let state = self.status(options.brief)?;
                let text = report::describe_status(obj(&state["summary"]));
                Ok(Outcome::said(state, text))
            }
            ["rebuild"] => Ok(Outcome::json(self.rebuild(options.now, context)?)),
            ["rebuild-now"] => {
                self.switch_now()?;
                Ok(Outcome::json(object! {"requested" => true}))
            }
            ["discover", workspace] => {
                let tools = self.tools();
                let settings = store.exclusive(|| tools.initialize(&|line| context.say(line)))?;
                let (Some(runtime), Some(flutter)) =
                    (text(&settings, "runtime"), text(&settings, "flutter"))
                else {
                    return fail("Installed source and release compiler are incomplete");
                };
                let runner =
                    Runner::new(Path::new(runtime), Path::new(flutter), store.file("cache"))?;
                let graph = Graph::read(&absolute(Path::new(workspace)))?;
                Ok(Outcome::json(
                    runner
                        .discover(&graph, &|line| context.say(line))?
                        .to_json(),
                ))
            }
            _ => store.exclusive(|| self.change(options, context)),
        }
    }

    /// The commands that change something, under the manager lock.
    fn change(&self, options: &Options, context: &Context) -> Result<Outcome> {
        let store = &self.store;
        let tools = self.tools();
        let settings = tools.initialize(&|line| context.say(line))?;
        let setting = |key: &str| text(&settings, key).map(str::to_owned);
        let repositories = self.repositories();
        let available = Available {
            store,
            repositories: &repositories,
        };
        let activation = self.activation();
        let source = |location: &str| {
            let kind = if options.local {
                SourceKind::Local
            } else if options.builtin {
                SourceKind::Builtin
            } else {
                SourceKind::Git
            };
            Source::new(
                kind,
                location,
                options.package_path(),
                options.reference.as_deref(),
            )
        };
        let enable = |name: &str| -> Result<Resolved> {
            let installed = store.installed()?;
            let Some(value) = installed.get(name) else {
                return fail(format!(
                    "{name} is not a plugin you have. See denial-plugins list."
                ));
            };
            let retained = Source::from_json(&value["source"])?;
            let pinned = if retained.kind == SourceKind::Git {
                value["revision"].as_str()
            } else {
                None
            };
            let plugin = repositories.resolve(&retained, pinned)?;
            store.select(&plugin)?;
            Ok(plugin)
        };
        let rest: Vec<&str> = options.rest.iter().map(String::as_str).collect();
        match rest.as_slice() {
            ["initialize"] | ["prepare"] => Ok(Outcome::json(settings.clone())),
            ["defaults"] => {
                available.defaults(setting("runtime").as_deref())?;
                Ok(Outcome::said(
                    store.selection()?,
                    "The plugins Denial comes with are selected. denial-plugins apply switches the desktop to them.",
                ))
            }
            ["refresh-catalog"] => Ok(Outcome::json(available.refresh(&settings)?)),
            ["off"] | ["restore"] => {
                context.say("Applying your desktop");
                Ok(Outcome::said(
                    activation.restore()?,
                    "The standard desktop is running. Your selection is kept: denial-plugins apply brings it back.",
                ))
            }
            ["undo"] | ["revert"] => {
                context.say("Applying your desktop");
                Ok(Outcome::said(
                    activation.revert()?,
                    "The plugins that ran before are running again.",
                ))
            }
            ["inspect", location] => Ok(Outcome::json(repositories.inspect(&source(location)?)?)),
            ["add", location] => {
                // A name from the list, a folder, or a Git URL.
                let flagged = options.local || options.builtin;
                let listed = if flagged || location.contains('/') {
                    None
                } else {
                    report::plugin_list(store, &available.list(&settings)?)?
                        .into_iter()
                        .find(|entry| text(entry, "name") == Some(location))
                };
                let plugin = if let Some(listed) = listed {
                    if listed.get("installed") == Some(&Value::Bool(true)) {
                        enable(location)?
                    } else {
                        repositories.resolve(&Source::from_json(&listed["source"])?, None)?
                    }
                } else if !flagged && Path::new(location).is_dir() {
                    repositories.resolve(
                        &Source::new(
                            SourceKind::Local,
                            &display(&absolute(Path::new(location))),
                            options.package_path(),
                            None,
                        )?,
                        None,
                    )?
                } else {
                    repositories.resolve(&source(location)?, None)?
                };
                store.select(&plugin)?;
                let text = format!(
                    "{} is selected. denial-plugins apply switches the desktop to it.",
                    plugin.name
                );
                Ok(Outcome::said(plugin.to_json(), text))
            }
            ["enable", name] => Ok(Outcome::json(enable(name)?.to_json())),
            ["remove", package] => {
                store.remove(package)?;
                Ok(Outcome::said(
                    store.selection()?,
                    format!(
                        "{package} is no longer selected. denial-plugins apply switches the desktop."
                    ),
                ))
            }
            [command @ ("plan" | "apply" | "update")] => {
                let (Some(runtime), Some(flutter), Some(engine_root)) = (
                    setting("runtime"),
                    setting("flutter"),
                    setting("engine-root"),
                ) else {
                    return fail("Installed source and release compiler are incomplete");
                };
                let check = *command == "plan";
                preflight::require_valid(&store.selection()?)?;
                if !check {
                    activation.require_support()?;
                }
                let mut arguments = object! {
                    "runtime" => runtime.clone(),
                    "flutter" => flutter.clone(),
                    "command" => *command,
                };
                if let Some(request) = context.id() {
                    arguments.insert("request".into(), request.into());
                }
                let id = store.create_job("plan", arguments)?;
                let pid = std::process::id();
                store.update_job(&id, object! {"phase" => "running", "pid" => pid, "processStart" => process_start(pid)})?;
                // Status shows the step also for a run started from a
                // terminal.
                let say = |phase: &str| {
                    context.say(phase);
                    let _ = store.update_job(&id, object! {"message" => phase});
                };
                let run = || -> Result<Outcome> {
                    let arguments = flutter_arguments(&settings);
                    let sdk = std::env::var_os("DENIAL_SDK_PATH").map(PathBuf::from);
                    let result = Planner {
                        store,
                        repositories: &repositories,
                    }
                    .plan(
                        &PlanRequest {
                            runtime: &absolute(Path::new(&runtime)),
                            flutter: &absolute(Path::new(&flutter)),
                            flutter_arguments: &arguments,
                            candidate: &id,
                            update: options.update || *command == "update",
                            rebuild_of: None,
                            sdk_root: sdk.as_deref(),
                        },
                        &say,
                    )?;
                    let mut text = None;
                    if !check {
                        let target = setting("engine-target").unwrap_or_default();
                        let platform = setting("platform").unwrap_or_default();
                        builder::build(
                            store,
                            &id,
                            &BuildRequest {
                                engine_root: &absolute(Path::new(&engine_root)),
                                engine_target: &target,
                                platform: &platform,
                            },
                            &say,
                        )?;
                        say("Applying your desktop");
                        let native = activation.activate(&id)?;
                        text = Some(
                            if native.get("plugin_engine_differs") == Some(&Value::Bool(true)) {
                                "Your plugins are running. They were built for another engine than this session runs, and loaded anyway."
                            } else {
                                "Your plugins are running."
                            },
                        );
                    }
                    store.update_job(
                        &id,
                        object! {"phase" => "succeeded", "candidate" => id.clone()},
                    )?;
                    Ok(Outcome {
                        result: Value::Object(result),
                        text: text.map(str::to_owned),
                    })
                };
                run().inspect_err(|error| {
                    let _ = store.update_job(
                        &id,
                        object! {"phase" => "failed", "error" => error.message.clone()},
                    );
                })
            }
            _ => Err(Error::new(format!(
                "Unknown command: {}\n\n{USAGE}",
                options.rest.join(" ")
            ))),
        }
    }

    /// Rebuilds the composition deniald ran before Denial was updated
    /// (PLUGIN_MANAGER.md, section 21). The manager lock is released while
    /// waiting for a quiet moment, so Plugins stays usable.
    pub fn rebuild(&self, now: bool, context: &Context) -> Result<Map> {
        let store = &self.store;
        let activation = self.activation();
        let built = store.exclusive(|| -> Result<Option<(String, String)>> {
            // Only a request made while this rebuild waits may skip the wait.
            let _ = std::fs::remove_file(rebuild::switch_now_request(store));
            let settings = self.tools().initialize(&|line| context.say(line))?;
            let Some(target) = Target::from_native(store, &activation.status())? else {
                return Ok(None);
            };
            let setting = |key: &str| text(&settings, key).map(str::to_owned);
            let (Some(runtime), Some(flutter), Some(engine_root)) = (
                setting("runtime"),
                setting("flutter"),
                setting("engine-root"),
            ) else {
                return fail("Installed source and release compiler are incomplete");
            };
            rebuild::require_matching_kit(Path::new(&runtime), &target)?;
            activation.require_support()?;
            let mut arguments = object! {
                "runtime" => runtime.clone(),
                "flutter" => flutter.clone(),
                "rebuildOf" => target.candidate.clone(),
                "command" => "rebuild",
            };
            if let Some(request) = context.id() {
                arguments.insert("request".into(), request.into());
            }
            let id = store.create_job("plan", arguments)?;
            let pid = std::process::id();
            store.update_job(
                &id,
                object! {"phase" => "running", "pid" => pid, "processStart" => process_start(pid)},
            )?;
            let run = || -> Result<()> {
                let repositories = self.repositories();
                let sdk = std::env::var_os("DENIAL_SDK_PATH").map(PathBuf::from);
                Planner {
                    store,
                    repositories: &repositories,
                }
                .plan(
                    &PlanRequest {
                        runtime: &absolute(Path::new(&runtime)),
                        flutter: &absolute(Path::new(&flutter)),
                        flutter_arguments: &flutter_arguments(&settings),
                        candidate: &id,
                        update: false,
                        rebuild_of: Some(&target.candidate),
                        sdk_root: sdk.as_deref(),
                    },
                    &|phase| {
                        context.say(phase);
                        let _ = store.update_job(&id, object! {"message" => phase});
                    },
                )?;
                let engine_target = setting("engine-target").unwrap_or_default();
                let platform = setting("platform").unwrap_or_default();
                builder::build(
                    store,
                    &id,
                    &BuildRequest {
                        engine_root: &absolute(Path::new(&engine_root)),
                        engine_target: &engine_target,
                        platform: &platform,
                    },
                    &|line| context.say(line),
                )?;
                store.update_job(
                    &id,
                    object! {"phase" => "succeeded", "candidate" => id.clone()},
                )
            };
            run().inspect_err(|error| {
                let _ = store.update_job(
                    &id,
                    object! {"phase" => "failed", "error" => error.message.clone()},
                );
            })?;
            Ok(Some((id, target.candidate)))
        })?;
        let Some((candidate, base)) = built else {
            return Ok(object! {"rebuilt" => false, "switched" => false});
        };
        let unchanged =
            object! {"rebuilt" => true, "switched" => false, "candidate" => candidate.clone()};
        if !now
            && !rebuild::wait_for_quiet_moment(
                store,
                &|| activation.status(),
                &base,
                &|line| context.say(line),
                &std::thread::sleep,
            )?
        {
            return Ok(unchanged);
        }
        let _lock = store.lock(Duration::from_secs(120))?;
        let _ = std::fs::remove_file(rebuild::switch_now_request(store));
        if !rebuild::waits_for_rebuild_of(store, &activation.status(), &base)? {
            return Ok(unchanged);
        }
        context.say("Applying your desktop");
        let native = activation.activate(&candidate)?;
        let mut switched = unchanged;
        switched.insert("switched".into(), true.into());
        switched.insert("native".into(), Value::Object(native));
        Ok(switched)
    }

    /// Started by deniald after an update left its plugins behind. It starts
    /// the rebuild and keeps one notification about it until nothing is left
    /// to do; then it waits for the requests it started.
    pub fn resume(&self) -> Result<()> {
        let store = &self.store;
        let Some(_lock) = store.try_named_lock("resume.lock")? else {
            // Another resume already keeps the user informed.
            return Ok(());
        };
        let activation = self.activation();
        let finish = || {
            while working() {
                std::thread::sleep(Duration::from_millis(250));
            }
        };
        let mut notifier = match DesktopNotifications::connect(Duration::from_secs(60)) {
            Ok(notifier) => notifier,
            Err(error) => {
                // Without notifications the rebuild still runs; Plugins
                // shows it.
                eprintln!("denial-plugins: notifications are unavailable: {error}");
                if Target::from_native(store, &activation.status())?.is_some() {
                    self.submit(&["rebuild"])?;
                }
                finish();
                return Ok(());
            }
        };
        // Settings opens on its Plugins page.
        let plugins =
            notifications::desktop_entry_command("org.denialwm.Settings.desktop", &|key| {
                std::env::var(key).ok()
            })
            .map(|mut command| {
                command.push("plugins".into());
                command
            });
        let open_plugins = |token: Option<&str>| {
            if let Some(plugins) = &plugins {
                let _ = notifications::launch_detached(plugins, token);
            }
        };
        let tools = self.tools();
        let kit_identity = if tools.available() {
            tools.identity().ok()
        } else {
            None
        };
        let clock = std::time::Instant::now;
        let outcome = Resume::new(Session {
            store,
            status: &|| activation.status(),
            submit: &|argv| self.submit(argv),
            notifier: &mut notifier,
            kit_identity,
            open_plugins: plugins
                .is_some()
                .then_some(&open_plugins as &dyn Fn(Option<&str>)),
            open_url: &|url, token| {
                let _ = notifications::launch_detached(&["xdg-open".into(), url.into()], token);
            },
            online: &notifications::network_connected,
            tick: Duration::from_secs(1),
            clock: &clock,
            sleep: &std::thread::sleep,
        })
        .run();
        finish();
        outcome
    }
}

fn flutter_arguments(settings: &Map) -> Vec<String> {
    util::strings(util::list(settings, "flutter-arguments"))
}
