//! What the plugin manager promises the user: applying counts only once
//! deniald confirms the start, undo and off keep the selection, a failed
//! start leaves the running desktop and says so, and a Denial update brings
//! the plugins back with one notification.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use denial_plugins::activation::Activation;
use denial_plugins::notifications::{Content, Event, Notifier};
use denial_plugins::object;
use denial_plugins::report;
use denial_plugins::resume::{Resume, Session};
use denial_plugins::store::{Store, process_start};
use denial_plugins::util::{Map, Result};
use serde_json::{Value, json};

fn map(value: Value) -> Map {
    value.as_object().cloned().unwrap()
}

/// A state folder, and a `denialctl` that answers what `answers` holds:
/// `ui OPERATION` prints `OPERATION.json`, else `status.json`. Every call
/// is written to `calls`.
struct Fixture {
    _folder: tempfile::TempDir,
    store: Store,
    control: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::new(folder.path().join("state"));
        let control = folder.path().join("control");
        std::fs::create_dir_all(&control).unwrap();
        let script = control.join("denialctl");
        std::fs::write(
            &script,
            "#!/bin/sh\nhere=$(dirname \"$0\")\nprintf '%s\\n' \"$*\" >>\"$here/calls\"\nif [ -f \"$here/$3.json\" ]; then cat \"$here/$3.json\"; else cat \"$here/status.json\"; fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _folder: folder,
            store,
            control,
        }
    }

    fn answer(&self, operation: &str, value: Value) {
        std::fs::write(
            self.control.join(format!("{operation}.json")),
            value.to_string(),
        )
        .unwrap();
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.control.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn activation(&self) -> Activation {
        Activation {
            store: self.store.clone(),
            denialctl: self.control.join("denialctl"),
        }
    }

    /// A built candidate as activation records it.
    fn built(&self, id: &str, roots: Value) -> String {
        let bundle = self.store.root.join("candidates").join(id).join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        let bundle = std::fs::canonicalize(&bundle)
            .unwrap()
            .display()
            .to_string();
        self.store
            .write(
                &format!("candidates/{id}/plan.json"),
                map(json!({"status": "built", "selectionRevision": 1, "roots": roots, "selections": {}, "ordering": {}})),
            )
            .unwrap();
        self.store
            .write(
                &format!("candidates/{id}/build.json"),
                map(json!({"status": "built", "bundle": bundle})),
            )
            .unwrap();
        bundle
    }
}

fn root(kind: &str, location: &str, revision: &str) -> Value {
    json!({"source": {"kind": kind, "location": location, "path": "."}, "name": Path::new(location).file_name().unwrap().to_str().unwrap().trim_end_matches(".git"), "revision": revision})
}

#[test]
fn apply_counts_only_once_deniald_runs_the_exact_bundle_healthy() {
    let fixture = Fixture::new();
    fixture.answer("status", json!({"available": false}));
    let activation = fixture.activation();
    // Nothing answers outside a session; an old session cannot load it.
    assert!(activation.require_support().is_err());
    fixture.answer("status", json!({"active_mode": "official_optimized"}));
    assert!(
        activation
            .require_support()
            .unwrap_err()
            .message
            .contains("Log out and back")
    );
    fixture.answer(
        "status",
        json!({"active_mode": "official_optimized", "capabilities": ["pluginBundleV2"]}),
    );
    activation.require_support().unwrap();

    let bundle = fixture.built(
        "one",
        json!({"panel": root("git", "https://example.org/panel.git", "new")}),
    );
    fixture.answer(
        "activate",
        json!({"active_mode": "custom_optimized", "plugin_bundle": bundle, "plugin_healthy": true}),
    );
    let native = activation.activate("one").unwrap();
    assert_eq!(
        fixture.calls().last().unwrap(),
        &format!("--json ui activate {bundle}")
    );
    // deniald is the only record of what runs.
    assert_eq!(
        activation.running_candidate(&native).as_deref(),
        Some("one")
    );
    assert!(!fixture.store.file("active.json").exists());
    assert_eq!(
        fixture.store.installed().unwrap()["panel"]["revision"],
        "new"
    );
    // A composition built elsewhere is never taken for a candidate.
    assert_eq!(
        activation.candidate_of(Some(&json!("/elsewhere/candidates/one/bundle"))),
        None
    );
}

#[test]
fn a_failed_start_keeps_the_running_desktop_and_says_so() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    store
        .select(&denial_plugins::source::Resolved {
            source: denial_plugins::source::Source::from_json(
                &json!({"kind": "git", "location": "https://example.org/panel.git", "path": "."}),
            )
            .unwrap(),
            name: "panel".into(),
            directory: "/cache/old".into(),
            revision: "old".into(),
            description: "Panel".into(),
        })
        .unwrap();
    let bundle = fixture.built(
        "updated",
        json!({"panel": root("git", "https://example.org/panel.git", "new")}),
    );
    fixture.answer("activate", json!({"active_mode": "custom_optimized", "plugin_bundle": bundle, "plugin_healthy": false, "error": "The shell showed nothing."}));
    let error = fixture.activation().activate("updated").unwrap_err();
    assert_eq!(
        error.message,
        "Your plugins did not start: The shell showed nothing."
    );
    // The confirmed revision stays the one to come back to.
    assert_eq!(store.installed().unwrap()["panel"]["revision"], "old");

    // Status reports the failed request, worded, with what to do next.
    let id = store
        .create_job("apply", object! {"argv" => vec!["apply"]})
        .unwrap();
    store
        .update_job(&id, object! {"phase" => "failed", "error" => format!("{} Denial kept the desktop it was running.", error.message)})
        .unwrap();
    let state = object! {
        "native" => json!({"active_mode": "official_optimized", "capabilities": ["pluginBundleV2"], "plugin_previous": null}),
        "selection" => store.selection().unwrap(),
        "jobs" => store.jobs().unwrap().into_iter().map(Value::Object).collect::<Vec<_>>(),
        "ready" => true,
    };
    let summary = report::summarize(store, &state).unwrap();
    assert_eq!(summary["desktop"]["code"], "standard");
    assert_eq!(summary["failure"]["code"], "did_not_start");
    assert_eq!(summary["next"]["code"], "apply");
    assert_eq!(summary["can"]["apply"], true);

    // A request whose process vanished is reported stopped, never busy for
    // ever.
    let lost = store
        .create_job("apply", object! {"argv" => vec!["apply"]})
        .unwrap();
    store
        .update_job(
            &lost,
            object! {"phase" => "running", "pid" => 2147483647u32, "processStart" => "impossible"},
        )
        .unwrap();
    assert_eq!(store.job(&lost).unwrap()["phase"], "interrupted");
    let history = report::history_report(&store.jobs().unwrap());
    assert_eq!(history["entries"][0]["code"], "stopped");
}

#[test]
fn undo_restores_the_selection_and_off_keeps_it() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let bundle = fixture.built("previous", json!({}));
    let mut plan = store.read("candidates/previous/plan.json").unwrap();
    plan.insert("selections".into(), json!({"contract": "provider"}));
    plan.insert(
        "ordering".into(),
        json!({"collection": ["second", "first"]}),
    );
    store.write("candidates/previous/plan.json", plan).unwrap();
    store
        .write("last-plan.json", object! {"id" => "current"})
        .unwrap();
    fixture.answer(
        "revert",
        json!({"plugin_healthy": true, "plugin_bundle": bundle}),
    );
    fixture.answer("restore", json!({"active_mode": "official_optimized"}));
    let activation = fixture.activation();
    activation.revert().unwrap();
    let selection = store.selection().unwrap();
    assert_eq!(selection["revision"], 1);
    assert_eq!(selection["selections"], json!({"contract": "provider"}));
    assert_eq!(
        selection["ordering"],
        json!({"collection": ["second", "first"]})
    );
    assert_eq!(store.read("last-plan.json").unwrap()["id"], "previous");
    activation.restore().unwrap();
    assert_eq!(store.read("last-plan.json").unwrap()["id"], "previous");
    assert_eq!(store.selection().unwrap()["revision"], 1);
}

// Resume.

#[derive(Default)]
struct Shown {
    shown: Vec<(u32, Content, u32)>,
    closed: Vec<u32>,
}

struct FakeNotifier {
    seen: Arc<Mutex<Shown>>,
    events: Receiver<Event>,
    next: u32,
}

impl Notifier for FakeNotifier {
    fn show(&mut self, content: &Content, replaces: u32) -> Result<u32> {
        let id = if replaces == 0 {
            self.next += 1;
            self.next
        } else {
            replaces
        };
        self.seen
            .lock()
            .unwrap()
            .shown
            .push((id, content.clone(), replaces));
        Ok(id)
    }

    fn close(&mut self, id: u32) -> Result<()> {
        self.seen.lock().unwrap().closed.push(id);
        Ok(())
    }

    fn next(&mut self, timeout: Duration) -> Option<Event> {
        self.events.recv_timeout(timeout).ok()
    }
}

/// What the test changes while resume runs.
#[derive(Clone)]
struct World {
    store: Store,
    native: Arc<Mutex<Map>>,
    unavailable: Arc<Mutex<u32>>,
    connected: Arc<Mutex<Option<bool>>>,
    submitted: Arc<Mutex<Vec<Vec<String>>>>,
    opened: Arc<Mutex<Vec<Option<String>>>>,
    seen: Arc<Mutex<Shown>>,
    events: Sender<Event>,
}

impl World {
    fn until(&self, what: impl Fn(&World) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !what(self) {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn shown(&self) -> Vec<(u32, Content, u32)> {
        self.seen.lock().unwrap().shown.clone()
    }

    fn job(&self) -> String {
        self.store.read("resume.json").unwrap()["job"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn emit(&self, event: Event) {
        self.events.send(event).unwrap();
    }

    fn set_native(&self, value: Value) {
        *self.native.lock().unwrap() = map(value);
    }
}

impl Clone for Shown {
    fn clone(&self) -> Self {
        Self {
            shown: self.shown.clone(),
            closed: self.closed.clone(),
        }
    }
}

/// Runs resume against `world` on this thread while `script` drives it
/// from another.
fn resume(fixture: &Fixture, world: &World, script: impl FnOnce(World) + Send + 'static) {
    let (sender, events) = channel();
    let mut notifier = FakeNotifier {
        seen: world.seen.clone(),
        events,
        next: 0,
    };
    let mut driver = world.clone();
    driver.events = sender;
    let driving = std::thread::spawn(move || script(driver));
    let status = || {
        let mut unavailable = world.unavailable.lock().unwrap();
        if *unavailable > 0 {
            *unavailable -= 1;
            return object! {"available" => false};
        }
        world.native.lock().unwrap().clone()
    };
    let submit = |argv: &[String]| {
        world.submitted.lock().unwrap().push(argv.to_vec());
        let id = world
            .store
            .create_job(&argv[0], object! {"argv" => argv.to_vec()})?;
        let pid = std::process::id();
        world.store.update_job(
            &id,
            object! {"phase" => "running", "pid" => pid, "processStart" => process_start(pid)},
        )?;
        Ok(id)
    };
    let open = |token: Option<&str>| world.opened.lock().unwrap().push(token.map(str::to_owned));
    let online = || *world.connected.lock().unwrap();
    let clock = Instant::now;
    Resume::new(Session {
        store: &fixture.store,
        status: &status,
        submit: &submit,
        notifier: &mut notifier,
        kit_identity: Some("kit".into()),
        open_plugins: Some(&open),
        open_url: &|_, _| {},
        online: &online,
        tick: Duration::from_millis(1),
        clock: &clock,
        sleep: &std::thread::sleep,
    })
    .run()
    .unwrap();
    driving.join().unwrap();
}

fn world(fixture: &Fixture) -> World {
    let base = fixture.built(
        "base",
        json!({"panel": root("git", "https://example.org/panel.git", "a")}),
    );
    World {
        store: fixture.store.clone(),
        native: Arc::new(Mutex::new(map(json!({
            "active_mode": "official_optimized",
            "operation": "idle",
            "plugin_healthy": false,
            "plugin_rebuild": {"reason": "source", "bundle": base, "version": "0.3.0"},
        })))),
        unavailable: Arc::default(),
        connected: Arc::default(),
        submitted: Arc::default(),
        opened: Arc::default(),
        seen: Arc::default(),
        events: channel().0,
    }
}

#[test]
fn resume_restores_the_plugins_and_thanks_the_user() {
    let fixture = Fixture::new();
    let world = world(&fixture);
    // The control socket may answer only once deniald's loop runs.
    *world.unavailable.lock().unwrap() = 3;
    resume(&fixture, &world, |world| {
        world.until(|world| !world.shown().is_empty());
        assert_eq!(
            *world.submitted.lock().unwrap(),
            [vec!["rebuild".to_owned()]]
        );
        let (id, progress, _) = world.shown()[0].clone();
        assert_eq!(progress.title, "Bringing back your plugins");
        assert!(progress.body.contains("Denial 0.3.0 is installed"));
        assert!(progress.body.contains("No need to log out"));
        assert!(progress.persistent);
        world.emit(Event::Action {
            id,
            key: "open".into(),
            token: Some("token".into()),
        });
        world.until(|world| !world.opened.lock().unwrap().is_empty());
        assert_eq!(*world.opened.lock().unwrap(), [Some("token".to_owned())]);

        world
            .store
            .update_job(&world.job(), object! {"phase" => "succeeded"})
            .unwrap();
        world.set_native(
            json!({"active_mode": "custom_optimized", "operation": "idle", "plugin_healthy": true}),
        );
        world.until(|world| world.shown().len() == 2);
        let (restored, content, replaces) = world.shown()[1].clone();
        assert_eq!(replaces, id);
        assert_eq!(content.title, "Your plugins are back");
        assert!(content.body.contains("now on Denial 0.3.0"));
        assert_eq!(content.actions, [("whats-new", "What's new")]);
        assert_eq!(
            world.store.read("resume.json").unwrap()["outcome"],
            "succeeded"
        );
        world.emit(Event::Closed { id: restored });
    });
}

#[test]
fn resume_offers_recovery_once_then_later_logins_only_remind() {
    let fixture = Fixture::new();
    let world = world(&fixture);
    resume(&fixture, &world, |world| {
        world.until(|world| !world.shown().is_empty());
        world
            .store
            .update_job(&world.job(), object! {"phase" => "failed", "error" => "panel requires denial_sdk ^0.2.0; selected SDK is 0.3.0."})
            .unwrap();
        world.until(|world| world.shown().len() == 2);
        let (failed, content, _) = world.shown()[1].clone();
        assert_eq!(content.title, "Your plugins couldn't be rebuilt");
        assert!(
            content.body.contains("may need an update")
                && content.body.contains("settings are safe")
        );
        let keys: Vec<&str> = content.actions.iter().map(|action| action.0).collect();
        assert_eq!(keys, ["retry", "update", "open", "default"]);
        assert_eq!(
            world.store.read("resume.json").unwrap()["outcome"],
            "failed"
        );
        world.emit(Event::Action {
            id: failed,
            key: "retry".into(),
            token: None,
        });
        world.until(|world| world.submitted.lock().unwrap().len() == 2);
        assert_eq!(world.submitted.lock().unwrap()[1], ["rebuild", "--now"]);
        world.until(|world| world.shown().len() == 3);
        assert_eq!(world.shown()[2].2, failed);
        world
            .store
            .update_job(
                &world.job(),
                object! {"phase" => "failed", "error" => "still broken"},
            )
            .unwrap();
        world.until(|world| world.shown().len() == 4);
        world.emit(Event::Closed {
            id: world.shown()[3].0,
        });
    });

    // The next login does not compile the same failure again.
    let later = World {
        seen: Arc::default(),
        ..world.clone()
    };
    resume(&fixture, &later, |world| {
        world.until(|world| !world.shown().is_empty());
        assert_eq!(world.shown()[0].1.title, "Your plugins are still paused");
        assert_eq!(world.submitted.lock().unwrap().len(), 2);
        world.emit(Event::Closed {
            id: world.shown()[0].0,
        });
    });
}

#[test]
fn resume_waits_for_a_connection_and_continues_by_itself() {
    let fixture = Fixture::new();
    let world = world(&fixture);
    resume(&fixture, &world, |world| {
        world.until(|world| !world.shown().is_empty());
        *world.connected.lock().unwrap() = Some(false);
        world
            .store
            .update_job(&world.job(), object! {"phase" => "failed", "error" => "SocketException: Failed host lookup: 'pub.dev'"})
            .unwrap();
        world.until(|world| world.shown().len() == 2);
        assert_eq!(
            world.shown()[1].1.title,
            "Your plugins are waiting for a connection"
        );
        assert_eq!(
            world.store.read("resume.json").unwrap()["outcome"],
            "offline"
        );
        *world.connected.lock().unwrap() = Some(true);
        world.until(|world| world.submitted.lock().unwrap().len() == 2);
        assert_eq!(world.submitted.lock().unwrap()[1], ["rebuild"]);
        world.until(|world| world.shown().len() == 3);
        assert!(world.shown()[2].1.body.contains("back online"));
        world
            .store
            .update_job(&world.job(), object! {"phase" => "succeeded"})
            .unwrap();
        world.set_native(json!({"active_mode": "custom_optimized", "plugin_healthy": true}));
        world.until(|world| world.shown().len() == 4);
        world.emit(Event::Closed {
            id: world.shown()[3].0,
        });
    });
}

#[test]
fn resume_says_plainly_when_the_composition_cannot_be_rebuilt() {
    let fixture = Fixture::new();
    let world = world(&fixture);
    world.set_native(json!({
        "active_mode": "official_optimized",
        "plugin_rebuild": {"bundle": "/elsewhere/candidates/base/bundle", "version": "development"},
    }));
    resume(&fixture, &world, |world| {
        world.until(|world| !world.shown().is_empty());
        let (id, content, _) = world.shown()[0].clone();
        assert!(world.submitted.lock().unwrap().is_empty());
        assert!(content.body.contains("can't be rebuilt automatically"));
        // A development build identity is never shown as a release.
        assert!(!content.body.contains("development"));
        let keys: Vec<&str> = content.actions.iter().map(|action| action.0).collect();
        assert_eq!(keys, ["open", "default"]);
        world.emit(Event::Closed { id });
    });
}
