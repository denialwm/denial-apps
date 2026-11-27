//! Keeps the user informed while a Denial update's plugin rebuild runs, in
//! one notification that changes only when the outcome does
//! (PLUGIN_MANAGER.md, section 21). The rebuild itself is an ordinary
//! request.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::notifications::{Content, Event, Notifier};
use crate::object;
use crate::rebuild::{BUILD_KIT_MISMATCH, Target, is_release};
use crate::store::Store;
use crate::util::{self, Kind, Map, Result, content_key, obj, text};

/// Why a rebuild after a Denial update did not finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Offline,
    Busy,
    Stopped,
    BuildTools,
    MissingBuiltin,
    /// The composition is not in this manager's history; only Apply helps.
    Unavailable,
    Plugins,
}

pub fn classify_rebuild_failure(error: &str) -> Problem {
    if error.contains("Another plugin manager operation is running") {
        return Problem::Busy;
    }
    if error.contains("worker exited") || error.contains("stopped unexpectedly") {
        return Problem::Stopped;
    }
    if error.contains("not built by this plugin manager")
        || error.contains("can no longer be rebuilt")
    {
        return Problem::Unavailable;
    }
    if error.contains(BUILD_KIT_MISMATCH)
        || error.contains("plugin build tools are missing")
        || error.contains("build tools are missing")
        || error.contains("Unsupported plugin build kit")
        || error.contains("Build kit input changed")
    {
        return Problem::BuildTools;
    }
    if error.contains("is not provided by this installed runtime") {
        return Problem::MissingBuiltin;
    }
    const OFFLINE: [&str; 13] = [
        "SocketException",
        "Failed host lookup",
        "Could not resolve host",
        "Could not resolve hostname",
        "Temporary failure in name resolution",
        "Network is unreachable",
        "Connection timed out",
        "Connection refused",
        "Connection reset",
        "HandshakeException",
        "ClientException",
        "Got socket error",
        "unable to access",
    ];
    if OFFLINE.iter().any(|sign| error.contains(sign)) {
        return Problem::Offline;
    }
    // The composition was confirmed before Denial changed. Anything else
    // that stops its rebuild almost always means a plugin needs an update.
    Problem::Plugins
}

/// The words the user reads. They promise what the system guarantees: the
/// plugins and their settings are kept, and no logout is needed.
pub struct Messages {
    pub version: String,
    pub has_git_plugins: bool,
}

const KEPT: &str = "Your plugins and their settings are safe.";
const OPEN_PLUGINS: (&str, &str) = ("open", "Open Plugins");
const TRY_AGAIN: (&str, &str) = ("retry", "Try again");
const CHECK_FOR_UPDATES: (&str, &str) = ("update", "Check for updates");

impl Messages {
    fn release(&self) -> bool {
        is_release(&self.version)
    }

    fn denial(&self) -> String {
        if self.release() {
            format!("Denial {}", self.version)
        } else {
            "this Denial build".into()
        }
    }

    fn open(can_open: bool) -> Vec<(&'static str, &'static str)> {
        if can_open {
            vec![OPEN_PLUGINS, ("default", "Open Plugins")]
        } else {
            Vec::new()
        }
    }

    fn recovery(
        &self,
        can_open: bool,
        retry: bool,
        updates: bool,
    ) -> Vec<(&'static str, &'static str)> {
        let mut actions = Vec::new();
        if retry {
            actions.push(TRY_AGAIN);
        }
        if updates && self.has_git_plugins {
            actions.push(CHECK_FOR_UPDATES);
        }
        actions.extend(Self::open(can_open));
        actions
    }

    fn progress(body: String, can_open: bool) -> Content {
        Content {
            title: "Bringing back your plugins".into(),
            body,
            actions: Self::open(can_open),
            resident: true,
            persistent: true,
        }
    }

    pub fn rebuilding(&self, can_open: bool) -> Content {
        let installed = if self.release() {
            format!("Denial {} is installed", self.version)
        } else {
            "Denial was updated".into()
        };
        Self::progress(
            format!(
                "{installed}. Your plugins and their settings are safe: they're being rebuilt in the background and will come back on their own. No need to log out."
            ),
            can_open,
        )
    }

    pub fn retrying(&self, can_open: bool) -> Content {
        Self::progress(
            format!(
                "Trying again for {}. Your desktop switches back as soon as they're ready.",
                self.denial()
            ),
            can_open,
        )
    }

    pub fn updating(&self, can_open: bool) -> Content {
        Self::progress(
            format!(
                "Looking for plugin updates for {}. Your desktop switches back as soon as they're ready.",
                self.denial()
            ),
            can_open,
        )
    }

    pub fn reconnected(&self, can_open: bool) -> Content {
        Self::progress(
            "You're back online. Your plugins will come back on their own as soon as they're ready.".into(),
            can_open,
        )
    }

    pub fn restored(&self) -> Content {
        Content {
            title: "Your plugins are back".into(),
            body: if self.release() {
                format!(
                    "Everything is as you left it, now on Denial {}. Thanks for waiting.",
                    self.version
                )
            } else {
                "Everything is as you left it. Thanks for waiting.".into()
            },
            actions: if self.release() {
                vec![("whats-new", "What's new")]
            } else {
                Vec::new()
            },
            resident: false,
            persistent: false,
        }
    }

    pub fn offline(&self, can_open: bool) -> Content {
        Content {
            title: "Your plugins are waiting for a connection".into(),
            body: format!(
                "Rebuilding them for {} needs a few downloads, and continues once you're online. {KEPT}",
                self.denial()
            ),
            actions: self.recovery(can_open, true, false),
            resident: true,
            persistent: true,
        }
    }

    pub fn failed(&self, problem: Problem, can_open: bool, plugin: Option<&str>) -> Content {
        Content {
            title: "Your plugins couldn't be rebuilt".into(),
            body: format!(
                "{} Meanwhile, Denial uses its standard desktop. {KEPT}",
                self.cause(problem, plugin)
            ),
            actions: self.recovery(
                can_open,
                problem != Problem::Unavailable,
                problem == Problem::Plugins,
            ),
            resident: true,
            persistent: true,
        }
    }

    pub fn still_paused(&self, can_open: bool) -> Content {
        Content {
            title: "Your plugins are still paused".into(),
            body: format!(
                "Denial uses its standard desktop until your plugins are rebuilt for {}. {KEPT}",
                self.denial()
            ),
            actions: self.recovery(can_open, true, true),
            resident: true,
            persistent: false,
        }
    }

    pub fn cause(&self, problem: Problem, plugin: Option<&str>) -> String {
        let denial = self.denial();
        match problem {
            Problem::BuildTools => format!(
                "The plugin build tools haven't been updated for {denial} yet. Finish updating the Denial Plugin Manager package, then try again."
            ),
            Problem::MissingBuiltin => format!(
                "{} is no longer included with {denial}. Choose what to use instead in Plugins.",
                plugin.unwrap_or("A built-in plugin")
            ),
            Problem::Unavailable => {
                "They can't be rebuilt automatically. Open Plugins and apply them again.".into()
            }
            Problem::Stopped | Problem::Offline => "The rebuild stopped before it finished.".into(),
            Problem::Busy => {
                "Another plugin operation kept running, so the rebuild could not start.".into()
            }
            Problem::Plugins => format!("One of them may need an update for {denial}."),
        }
    }

    pub fn release_notes(&self) -> String {
        format!(
            "https://github.com/denialwm/denial/releases/tag/v{}",
            self.version
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Rebuilding,
    Failed,
    Offline,
    Restored,
}

const OPERATIONS: [&str; 3] = ["rebuild", "apply", "update"];
const OFFLINE_RETRIES: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(300),
];

/// What the resume loop works with; tests give their own.
pub struct Session<'a> {
    pub store: &'a Store,
    pub status: &'a dyn Fn() -> Map,
    /// Starts a request; returns its job.
    pub submit: &'a dyn Fn(&[String]) -> Result<String>,
    pub notifier: &'a mut dyn Notifier,
    /// The installed build kit; one automatic attempt is made per kit and
    /// composition.
    pub kit_identity: Option<String>,
    /// Present only when Plugins is installed and can be opened.
    pub open_plugins: Option<&'a dyn Fn(Option<&str>)>,
    pub open_url: &'a dyn Fn(&str, Option<&str>),
    pub online: &'a dyn Fn() -> Option<bool>,
    pub tick: Duration,
    pub clock: &'a dyn Fn() -> Instant,
    pub sleep: &'a dyn Fn(Duration),
}

pub struct Resume<'a> {
    session: Session<'a>,
    id: u32,
    open: bool,
    job: Option<String>,
    argv: Vec<String>,
    state: State,
    busy_retries: u32,
    offline_attempts: usize,
    retry_at: Option<Instant>,
    was_online: Option<bool>,
    unavailable: u32,
    next_check: Option<Instant>,
    messages: Messages,
    key: String,
}

impl<'a> Resume<'a> {
    pub fn new(session: Session<'a>) -> Self {
        Self {
            session,
            id: 0,
            open: false,
            job: None,
            argv: vec!["rebuild".into()],
            state: State::Rebuilding,
            busy_retries: 0,
            offline_attempts: 0,
            retry_at: None,
            was_online: None,
            unavailable: 0,
            next_check: None,
            messages: Messages {
                version: String::new(),
                has_git_plugins: false,
            },
            key: String::new(),
        }
    }

    fn can_open(&self) -> bool {
        self.session.open_plugins.is_some()
    }

    /// Returns when nothing remains for the user to act on.
    pub fn run(mut self) -> Result<()> {
        // deniald starts this before its event loop answers control requests.
        let mut native = (self.session.status)();
        let mut attempt = 0;
        while native.get("available") == Some(&Value::Bool(false)) && attempt < 60 {
            (self.session.sleep)(self.session.tick);
            native = (self.session.status)();
            attempt += 1;
        }
        let Some(waiting) = native
            .get("plugin_rebuild")
            .and_then(Value::as_object)
            .cloned()
        else {
            return Ok(());
        };
        let store = self.session.store;
        let (target, unavailable) = match Target::from_native(store, &native) {
            Ok(target) => (target, None),
            Err(error) if error.kind == Kind::Composition => (None, Some(error.message)),
            Err(error) => return Err(error),
        };
        let plan = match &target {
            Some(target) => store.read(&format!("candidates/{}/plan.json", target.candidate))?,
            None => Map::new(),
        };
        self.messages = Messages {
            version: text(&waiting, "version").unwrap_or("").to_owned(),
            has_git_plugins: obj(plan.get("roots").unwrap_or(&Value::Null))
                .values()
                .any(|root| root["source"]["kind"] == "git"),
        };
        self.key = content_key(&Value::Object(object! {
            "kit" => self.session.kit_identity.clone(),
            "bundle" => waiting.get("bundle").cloned().unwrap_or(Value::Null),
        }));
        if let Some(unavailable) = unavailable {
            self.show_failure(&unavailable, Problem::Unavailable)?;
            return self.run_loop();
        }
        let ledger = store.read("resume.json")?;
        let jobs = store.jobs()?;
        let running = jobs.iter().find(|job| {
            OPERATIONS.contains(&text(job, "operation").unwrap_or(""))
                && job["arguments"]["argv"].is_array()
                && ["queued", "running"].contains(&text(job, "phase").unwrap_or(""))
        });
        if let Some(running) = running {
            self.job = text(running, "id").map(str::to_owned);
            self.argv = util::strings(
                running["arguments"]["argv"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice),
            );
            self.show(self.messages.rebuilding(self.can_open()))?;
        } else if text(&ledger, "key") == Some(self.key.as_str())
            && text(&ledger, "outcome") == Some("failed")
        {
            // One automatic attempt per installed build kit and composition.
            self.state = State::Failed;
            self.show(self.messages.still_paused(self.can_open()))?;
        } else {
            self.start(&["rebuild"])?;
            self.show(self.messages.rebuilding(self.can_open()))?;
        }
        self.run_loop()
    }

    fn start(&mut self, argv: &[&str]) -> Result<()> {
        self.argv = argv.iter().map(|argument| (*argument).to_owned()).collect();
        self.job = Some((self.session.submit)(&self.argv)?);
        self.state = State::Rebuilding;
        self.record("running")
    }

    fn restart(&mut self) -> Result<()> {
        let argv = self.argv.clone();
        self.job = Some((self.session.submit)(&argv)?);
        self.state = State::Rebuilding;
        self.record("running")
    }

    fn record(&self, outcome: &str) -> Result<()> {
        self.session.store.write(
            "resume.json",
            object! {
                "key" => self.key.clone(),
                "job" => self.job.clone(),
                "outcome" => outcome,
                "updated" => util::now(),
            },
        )
    }

    fn show(&mut self, content: Content) -> Result<()> {
        let replaces = if self.open { self.id } else { 0 };
        self.id = self.session.notifier.show(&content, replaces)?;
        self.open = true;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if self.open {
            self.session.notifier.close(self.id)?;
        }
        self.open = false;
        Ok(())
    }

    fn run_loop(&mut self) -> Result<()> {
        let started = (self.session.clock)();
        loop {
            if let Some(event) = self.session.notifier.next(self.session.tick) {
                if event.id() != self.id {
                    continue;
                }
                match event {
                    Event::Closed { .. } => {
                        self.open = false;
                        // A dismissed progress notification still deserves
                        // its outcome.
                        if self.state != State::Rebuilding {
                            return Ok(());
                        }
                    }
                    Event::Action { key, token, .. } => self.act(&key, token.as_deref())?,
                }
                continue;
            }
            match self.state {
                State::Rebuilding => self.observe_job()?,
                State::Failed | State::Offline => {
                    // Asking deniald spawns a process; a few seconds' delay
                    // is fine.
                    let now = (self.session.clock)();
                    if self.next_check.is_some_and(|next| now < next) {
                        continue;
                    }
                    self.next_check = Some(now + self.session.tick * 10);
                    if self.resolved_elsewhere()? {
                        return Ok(());
                    }
                    if self.state == State::Offline {
                        self.maybe_retry_online()?;
                    }
                }
                State::Restored => {
                    // The confirmation expires by itself; never linger
                    // without it.
                    if !self.open || (self.session.clock)() - started > Duration::from_secs(3600) {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn act(&mut self, key: &str, token: Option<&str>) -> Result<()> {
        match key {
            "open" | "default" => {
                if let Some(open) = self.session.open_plugins {
                    open(token);
                }
            }
            "whats-new" => (self.session.open_url)(&self.messages.release_notes(), token),
            "retry" if self.state != State::Rebuilding => {
                self.start(&["rebuild", "--now"])?;
                self.show(self.messages.retrying(self.can_open()))?;
            }
            "update" if self.state != State::Rebuilding => {
                self.start(&["update"])?;
                self.show(self.messages.updating(self.can_open()))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn observe_job(&mut self) -> Result<()> {
        let Some(id) = self.job.clone() else {
            return Ok(());
        };
        let job = self.session.store.job(&id)?;
        let phase = text(&job, "phase").unwrap_or("");
        if ["queued", "running"].contains(&phase) {
            return Ok(());
        }
        let native = (self.session.status)();
        if !native.get("plugin_rebuild").is_some_and(Value::is_object) {
            if text(&native, "active_mode") == Some("custom_optimized")
                && native.get("plugin_healthy") == Some(&Value::Bool(true))
            {
                self.record("succeeded")?;
                self.state = State::Restored;
                self.show(self.messages.restored())?;
            } else {
                // The user chose the standard desktop meanwhile; respect it
                // quietly.
                self.record("superseded")?;
                self.close()?;
                self.state = State::Restored;
            }
            return Ok(());
        }
        let error = if phase == "succeeded" {
            "The rebuilt plugins were not switched on".to_owned()
        } else {
            match job.get("error") {
                Some(Value::String(error)) => error.clone(),
                Some(Value::Null) | None => "The rebuild stopped unexpectedly".into(),
                Some(other) => other.to_string(),
            }
        };
        let problem = classify_rebuild_failure(&error);
        if problem == Problem::Busy && self.busy_retries < 3 {
            self.busy_retries += 1;
            (self.session.sleep)(self.session.tick * 5);
            return self.restart();
        }
        if problem == Problem::Offline {
            self.record("offline")?;
            self.state = State::Offline;
            self.offline_attempts += 1;
            self.retry_at = OFFLINE_RETRIES
                .get(self.offline_attempts - 1)
                .map(|delay| (self.session.clock)() + *delay);
            self.was_online = (self.session.online)();
            return self.show(self.messages.offline(self.can_open()));
        }
        self.show_failure(&error, problem)
    }

    fn show_failure(&mut self, error: &str, problem: Problem) -> Result<()> {
        self.record("failed")?;
        self.state = State::Failed;
        let plugin = error.lines().find_map(|line| {
            let head = &line[..line.find(" is not provided by this installed runtime")?];
            (!head.is_empty() && !head.contains(char::is_whitespace)).then_some(head)
        });
        self.show(self.messages.failed(problem, self.can_open(), plugin))
    }

    /// The user may have fixed it in Plugins, or chosen the standard
    /// desktop. A session that stays unavailable has ended; nothing is left
    /// to show.
    fn resolved_elsewhere(&mut self) -> Result<bool> {
        let native = (self.session.status)();
        if native.get("available") == Some(&Value::Bool(false)) {
            self.unavailable += 1;
            return Ok(self.unavailable >= 6);
        }
        self.unavailable = 0;
        if native.get("plugin_rebuild").is_some_and(Value::is_object) {
            return Ok(false);
        }
        self.close()?;
        Ok(true)
    }

    /// Retries by itself when the connection comes back, or after a few
    /// growing delays when nobody can tell or downloads failed while online.
    fn maybe_retry_online(&mut self) -> Result<()> {
        let connected = (self.session.online)();
        let reconnected = connected == Some(true) && self.was_online == Some(false);
        self.was_online = connected.or(self.was_online);
        let due = self.retry_at.is_some_and(|at| (self.session.clock)() >= at);
        // Known offline: wait for the connection instead of the timer.
        if !reconnected && (connected == Some(false) || !due) {
            return Ok(());
        }
        self.start(&["rebuild"])?;
        self.show(self.messages.reconnected(self.can_open()))
    }
}
