//! The manager's durable state in `$XDG_STATE_HOME/denial/plugins`: the
//! selection, what is installed, job records, candidates and caches. Every
//! record is a JSON object with `schema: 1`, the same files the Dart manager
//! wrote. What runs is deniald's record, never one kept here.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::object;
use crate::source::{Kind as SourceKind, Resolved};
use crate::util::{
    self, Error, Map, Result, canonical, encode, fail, file_digest, is_word, obj, sha256,
};

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

/// The manager's lock, held while it lives.
pub struct Lock {
    _file: File,
}

/// What locking refuses with while another operation runs.
pub const BUSY: &str = "Another plugin manager operation is running. Wait for its job to finish.";

/// Takes an exclusive lock on `file` that conflicts within this process too,
/// and with the record locks other processes take on the same file.
fn try_lock(file: &File) -> std::io::Result<bool> {
    // SAFETY: flock is plain data; zero is a valid value for every field.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    // SAFETY: the descriptor is open for the call, and lock is initialised.
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &lock) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EAGAIN) | Some(libc::EACCES) => Ok(false),
        _ => Err(error),
    }
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$XDG_STATE_HOME/denial/plugins`.
    pub fn default_root() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
            });
        state.join("denial/plugins")
    }

    pub fn file(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// A record, or an empty one when there is none.
    pub fn read(&self, relative: &str) -> Result<Map> {
        self.read_or(relative, Map::new())
    }

    pub fn read_or(&self, relative: &str, fallback: Map) -> Result<Map> {
        let path = self.file(relative);
        if !path.exists() {
            return Ok(fallback);
        }
        let value = util::read_json_map(&path)?;
        if value.get("schema") != Some(&Value::from(1)) {
            return fail(format!("Unsupported state schema in {}", path.display()));
        }
        Ok(value)
    }

    /// Replaces a record at once: written beside it, then renamed.
    pub fn write(&self, relative: &str, value: Map) -> Result<()> {
        let path = self.file(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::file(parent, &error))?;
        }
        let mut record = object! {"schema" => 1};
        record.extend(value);
        let temporary = PathBuf::from(format!("{}.{}.tmp", path.display(), std::process::id()));
        let text = format!("{}\n", util::pretty(&Value::Object(record)));
        {
            use std::io::Write;
            let mut file =
                File::create(&temporary).map_err(|error| Error::file(&temporary, &error))?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, &path).map_err(|error| Error::file(&path, &error))?;
        Ok(())
    }

    /// The manager lock. An operation that resumes after waiting may `wait`
    /// briefly for a short one, such as one Plugins started, to end.
    pub fn lock(&self, wait: Duration) -> Result<Lock> {
        std::fs::create_dir_all(&self.root).map_err(|error| Error::file(&self.root, &error))?;
        let path = self.file("manager.lock");
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|error| Error::file(&path, &error))?;
        let deadline = Instant::now() + wait;
        loop {
            if try_lock(&file)? {
                return Ok(Lock { _file: file });
            }
            if Instant::now() >= deadline {
                return fail(BUSY);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// Runs `action` under the manager lock.
    pub fn exclusive<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let _lock = self.lock(Duration::ZERO)?;
        action()
    }

    /// Takes `name`'s lock if nobody holds it; None when someone does.
    pub fn try_named_lock(&self, name: &str) -> Result<Option<Lock>> {
        std::fs::create_dir_all(&self.root).map_err(|error| Error::file(&self.root, &error))?;
        let path = self.file(name);
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|error| Error::file(&path, &error))?;
        Ok(try_lock(&file)?.then_some(Lock { _file: file }))
    }

    pub fn selection(&self) -> Result<Map> {
        self.read_or(
            "selection.json",
            object! {
                "schema" => 1,
                "revision" => 0,
                "roots" => Map::new(),
                "selections" => Map::new(),
                "ordering" => Map::new(),
            },
        )
    }

    /// The saved selection with `roots` and a new revision.
    pub fn write_selection(&self, selection: &Map, roots: Map) -> Result<()> {
        let mut next = selection.clone();
        let revision = next.get("revision").and_then(Value::as_i64).unwrap_or(0);
        next.insert("roots".into(), Value::Object(roots));
        next.insert("revision".into(), Value::from(revision + 1));
        self.write("selection.json", next)
    }

    /// The plugins this manager ever resolved, by name.
    pub fn installed(&self) -> Result<Map> {
        Ok(obj(self
            .read("installed.json")?
            .get("plugins")
            .unwrap_or(&Value::Null))
        .clone())
    }

    pub fn select(&self, plugin: &Resolved) -> Result<()> {
        let state = self.selection()?;
        let mut roots = obj(&state["roots"]).clone();
        if let Some(existing) = roots.get(&plugin.name) {
            let source = &existing["source"];
            let both_builtin =
                plugin.source.kind == SourceKind::Builtin && source["kind"] == "builtin";
            if !both_builtin && encode(source) != encode(&Value::Object(plugin.source.to_json())) {
                return fail(format!(
                    "{} already has a different source; remove it before replacing its source",
                    plugin.name
                ));
            }
        }
        let mut known = self.installed()?;
        known.insert(plugin.name.clone(), Value::Object(plugin.to_json()));
        self.write("installed.json", object! {"plugins" => known})?;
        roots.insert(plugin.name.clone(), Value::Object(plugin.to_json()));
        self.write_selection(&state, roots)
    }

    pub fn remove(&self, package: &str) -> Result<()> {
        let state = self.selection()?;
        let mut roots = obj(&state["roots"]).clone();
        if roots.shift_remove(package).is_none() {
            return fail(format!(
                "{package} is not a selected root. Required dependencies follow their dependent roots."
            ));
        }
        self.write_selection(&state, roots)
    }

    pub fn create_job(&self, operation: &str, arguments: Map) -> Result<String> {
        let stamp = util::micros_now();
        let pid = std::process::id();
        let id = format!("{stamp}-{pid}");
        self.write(
            &format!("jobs/{id}.json"),
            object! {
                "id" => id.clone(),
                "operation" => operation,
                "arguments" => arguments,
                "phase" => "queued",
                "created" => util::iso_time(stamp),
                "ownerPid" => pid,
                "ownerStart" => process_start(pid),
            },
        )?;
        Ok(id)
    }

    pub fn update_job(&self, id: &str, updates: Map) -> Result<()> {
        validate_id(id)?;
        let relative = format!("jobs/{id}.json");
        let mut job = self.read(&relative)?;
        job.extend(updates);
        job.insert("updated".into(), Value::from(util::now()));
        self.write(&relative, job)
    }

    /// One job's status, including whether its process ended unrecorded.
    pub fn job(&self, id: &str) -> Result<Map> {
        validate_id(id)?;
        Ok(self.job_status(self.read(&format!("jobs/{id}.json"))?))
    }

    /// The 30 newest jobs, the newest first.
    pub fn jobs(&self) -> Result<Vec<Map>> {
        let directory = self.file("jobs");
        let Ok(listing) = std::fs::read_dir(&directory) else {
            return Ok(Vec::new());
        };
        let mut names: Vec<String> = listing
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort_by(|a, b| b.cmp(a));
        names
            .into_iter()
            .take(30)
            .map(|name| Ok(self.job_status(self.read(&format!("jobs/{name}"))?)))
            .collect()
    }

    fn job_status(&self, job: Map) -> Map {
        let phase = job.get("phase").and_then(Value::as_str).unwrap_or("");
        if phase != "running" && phase != "queued" {
            return job;
        }
        let id = job.get("id").and_then(Value::as_str).unwrap_or("");
        let spawn = self.read(&format!("jobs/{id}.spawn")).unwrap_or_default();
        // A process is judged by the start time recorded beside it: its own
        // record, else the one that started it, else the client that asked.
        let (process, expected) = if let Some(pid) = job.get("pid").and_then(Value::as_u64) {
            (Some(pid), job.get("processStart").and_then(Value::as_str))
        } else if let Some(pid) = spawn.get("pid").and_then(Value::as_u64) {
            (Some(pid), spawn.get("processStart").and_then(Value::as_str))
        } else {
            (
                job.get("ownerPid").and_then(Value::as_u64),
                job.get("ownerStart").and_then(Value::as_str),
            )
        };
        let Some(process) = process else { return job };
        let actual = process_start(process as u32);
        let gone = match expected {
            None => actual.is_none(),
            Some(expected) => actual.as_deref() != Some(expected),
        };
        if !gone {
            return job;
        }
        let mut job = job;
        job.insert("phase".into(), "interrupted".into());
        job.insert(
            "error".into(),
            "The worker exited before completion was recorded. Check the native composition status before retrying."
                .into(),
        );
        job
    }
}

/// When process `pid` started, in clock ticks since boot; None when it is
/// gone. Together with the pid it names one process.
pub fn process_start(pid: u32) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &text[text.rfind(')')? + 2..];
    rest.split(' ').nth(19).map(str::to_owned)
}

pub fn validate_id(id: &str) -> Result<()> {
    if is_word(id) {
        Ok(())
    } else {
        fail("Invalid candidate/job identity")
    }
}

/// Checksums of files that did not change since they were last read: size,
/// modification and change times and mode must all match. Build
/// acceleration for trusted local inputs, not a security boundary.
pub struct DigestCache {
    store: Store,
    entries: Map,
    dirty: bool,
}

impl DigestCache {
    pub fn new(store: &Store) -> Self {
        let entries = store
            .read("cache/file-digests.json")
            .ok()
            .and_then(|record| record.get("files").and_then(Value::as_object).cloned())
            .unwrap_or_default();
        Self {
            store: store.clone(),
            entries,
            dirty: false,
        }
    }

    pub fn digest(&mut self, file: &Path) -> Result<String> {
        use std::os::unix::fs::MetadataExt;
        let canonical = canonical(file)?;
        let stamp = |path: &Path| -> Result<String> {
            let stat = std::fs::metadata(path).map_err(|error| Error::file(path, &error))?;
            Ok(format!(
                "{}:{}:{}:{}",
                stat.size(),
                stat.mtime() * 1_000_000 + stat.mtime_nsec() / 1000,
                stat.ctime() * 1_000_000 + stat.ctime_nsec() / 1000,
                stat.mode()
            ))
        };
        let before = stamp(&canonical)?;
        let key = canonical.display().to_string();
        if let Some(cached) = self.entries.get(&key).and_then(Value::as_object)
            && cached.get("stamp").and_then(Value::as_str) == Some(before.as_str())
            && let Some(digest) = cached.get("digest").and_then(Value::as_str)
        {
            return Ok(digest.to_owned());
        }
        let result = file_digest(&canonical)?;
        if stamp(&canonical)? != before {
            return fail(format!(
                "Input changed while checking {}; try again",
                canonical.display()
            ));
        }
        self.entries.insert(
            key,
            Value::Object(object! {"stamp" => before, "digest" => result.clone()}),
        );
        self.dirty = true;
        Ok(result)
    }

    pub fn tree(&mut self, directory: &Path, ignored: &[&str]) -> Result<String> {
        util::tree_digest(directory, ignored, &mut |path| self.digest(path))
    }

    pub fn save(&self) -> Result<()> {
        if self.dirty {
            self.store.write(
                "cache/file-digests.json",
                object! {"files" => self.entries.clone()},
            )?;
        }
        Ok(())
    }
}

/// The identity of a source description, as Dart's manager computed it.
pub fn identity(value: &Value) -> String {
    sha256(encode(value).as_bytes())
}
