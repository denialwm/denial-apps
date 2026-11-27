//! What every part of the manager shares: its error, JSON records, digests,
//! times, paths and running other programs.

use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub type Map = serde_json::Map<String, Value>;
pub type Result<T> = std::result::Result<T, Error>;

/// What kind of failure an error is. Most are the manager's own sentences;
/// a missing or unreadable file and a malformed declaration are told apart
/// where the manager treats them differently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Composition,
    FileSystem,
    Format,
}

/// A failure, with the sentence the user reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: Kind,
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Composition,
            message: message.into(),
        }
    }

    pub fn format(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Format,
            message: message.into(),
        }
    }

    pub fn file(path: &Path, error: &std::io::Error) -> Self {
        Self {
            kind: Kind::FileSystem,
            message: format!("{error}: {}", path.display()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: Kind::FileSystem,
            message: error.to_string(),
        }
    }
}

/// Fails with `message`.
pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::new(message))
}

/// The object `value` is, or an empty one.
pub fn obj(value: &Value) -> &Map {
    static EMPTY: OnceLock<Map> = OnceLock::new();
    value
        .as_object()
        .unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}

/// The string at `key`, if it is one.
pub fn text<'a>(map: &'a Map, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

/// The list at `key`, or an empty one.
pub fn list<'a>(map: &'a Map, key: &str) -> &'a [Value] {
    map.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The strings of a list, skipping anything else.
pub fn strings(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

/// Builds an object from pairs, in their order.
#[macro_export]
macro_rules! object {
    ($($key:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut map = $crate::util::Map::new();
        $(map.insert(($key).to_string(), serde_json::Value::from($value));)*
        map
    }};
}

/// Compact JSON, as Dart's `jsonEncode` writes it.
pub fn encode(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// JSON with two-space indentation, as the manager's records are written.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// A key that ignores the order of object keys and keeps list order.
pub fn content_key(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|key| (key.clone(), sorted(&map[key])))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    sha256(encode(&sorted(value)).as_bytes())
}

/// The checksum of a file.
pub fn file_digest(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|error| Error::file(path, &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| Error::file(path, &error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Fingerprints files, relative names and link targets of a tree, sorted by
/// path. Callers freeze local inputs first: a Git revision alone cannot name
/// uncommitted work.
pub fn tree_digest(
    root: &Path,
    ignored: &[&str],
    digest: &mut dyn FnMut(&Path) -> Result<String>,
) -> Result<String> {
    let mut entries = Vec::new();
    fn visit(directory: &Path, ignored: &[&str], entries: &mut Vec<PathBuf>) -> Result<()> {
        let listing =
            std::fs::read_dir(directory).map_err(|error| Error::file(directory, &error))?;
        for entry in listing {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if ignored.contains(&name) {
                continue;
            }
            let directory = std::fs::symlink_metadata(&path)?.is_dir();
            entries.push(path.clone());
            if directory {
                visit(&path, ignored, entries)?;
            }
        }
        Ok(())
    }
    visit(root, ignored, &mut entries)?;
    entries.sort_by(|a, b| {
        a.as_os_str()
            .as_encoded_bytes()
            .cmp(b.as_os_str().as_encoded_bytes())
    });
    let mut hasher = Sha256::new();
    for entry in entries {
        let relative = entry.strip_prefix(root).unwrap_or(&entry);
        hasher.update(relative.as_os_str().as_encoded_bytes());
        hasher.update([0]);
        let metadata = std::fs::symlink_metadata(&entry)?;
        let line = if metadata.is_symlink() {
            format!("link:{}\n", std::fs::read_link(&entry)?.display())
        } else if metadata.is_file() {
            format!("file:{}\n", digest(&entry)?)
        } else {
            "directory\n".to_owned()
        };
        hasher.update(line.as_bytes());
    }
    Ok(hex(&hasher.finalize()))
}

/// [`tree_digest`] reading every file.
pub fn plain_tree_digest(root: &Path) -> Result<String> {
    tree_digest(root, &[], &mut |path| file_digest(path))
}

pub fn micros_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_micros())
        .unwrap_or_default()
}

/// A UTC time as the manager writes it: `2026-11-18T10:00:00.123456Z`.
pub fn iso_time(micros: u128) -> String {
    let seconds = (micros / 1_000_000) as i64;
    let fraction = micros % 1_000_000;
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Civil date of a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{fraction:06}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

pub fn now() -> String {
    iso_time(micros_now())
}

/// `path` without `.` and with `..` taken back, as Dart's `normalize`.
pub fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            other => result.push(other),
        }
    }
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

/// `path` as an absolute, normalized path.
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&std::env::current_dir().unwrap_or_default().join(path))
    }
}

/// Whether `child` lies inside `parent`, not being it.
pub fn is_within(parent: &Path, child: &Path) -> bool {
    let parent = normalize(parent);
    let child = normalize(child);
    child != parent && child.starts_with(&parent)
}

/// `path` relative to `from`, both made absolute.
pub fn relative(path: &Path, from: &Path) -> PathBuf {
    let path = absolute(path);
    let from = absolute(from);
    let path: Vec<_> = path.components().collect();
    let from: Vec<_> = from.components().collect();
    let common = path.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for component in &path[common..] {
        result.push(component);
    }
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

/// The real path, links resolved.
pub fn canonical(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|error| Error::file(path, &error))
}

pub fn display(path: &Path) -> String {
    path.display().to_string()
}

pub fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `value` is a plain identity word: letters, digits, `_` and `-`.
pub fn is_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A full Git commit identity.
pub fn is_commit(value: &str) -> bool {
    (40..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A new, empty directory in `parent` named `prefix` and something unique.
pub fn temporary_directory(parent: &Path, prefix: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(parent).map_err(|error| Error::file(parent, &error))?;
    for attempt in 0..1000u32 {
        let candidate = parent.join(format!(
            "{prefix}{:x}{:x}{attempt}",
            std::process::id(),
            micros_now() % 0xffff_ffff
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::file(&candidate, &error)),
        }
    }
    fail(format!(
        "Cannot create a temporary directory in {}",
        parent.display()
    ))
}

/// Removes a tree that may hold read-only folders.
pub fn remove_tree(path: &Path) {
    if std::fs::remove_dir_all(path).is_ok() {
        return;
    }
    let _ = Command::new("chmod")
        .args(["-R", "u+rwX", "--"])
        .arg(path)
        .stderr(Stdio::null())
        .status();
    let _ = std::fs::remove_dir_all(path);
}

static SEARCH_PATH: OnceLock<std::ffi::OsString> = OnceLock::new();

/// Puts `folders` before PATH for the programs `run` starts: what the
/// packaging names, such as Nix's git. Set once per process.
pub fn search_first(folders: &[PathBuf]) {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let all = folders
        .iter()
        .cloned()
        .chain(std::env::split_paths(&current));
    if let Ok(joined) = std::env::join_paths(all) {
        let _ = SEARCH_PATH.set(joined);
    }
}

/// Runs `program`, and fails with what it said when it fails. Git never asks
/// for credentials. `output` receives each line of both streams as it comes.
pub fn run(
    program: &str,
    arguments: &[String],
    directory: Option<&Path>,
    output: Option<&mut dyn FnMut(&str)>,
) -> Result<String> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    if let Some(path) = SEARCH_PATH.get() {
        command.env("PATH", path);
    }
    let mut child = command
        .spawn()
        .map_err(|error| Error::new(format!("{program} failed to start: {error}")))?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let (lines, received) = std::sync::mpsc::channel::<(bool, String)>();
    // Both pipes drain at once: a verbose compiler would otherwise block on a
    // full stderr while its stdout is read.
    let readers = [
        (true, Box::new(stdout) as Box<dyn Read + Send>),
        (false, Box::new(stderr) as Box<dyn Read + Send>),
    ]
    .map(|(out, stream)| {
        let lines = lines.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).split(b'\n') {
                let Ok(line) = line else { break };
                let line = String::from_utf8_lossy(&line).into_owned();
                if lines.send((out, line)).is_err() {
                    break;
                }
            }
        })
    });
    drop(lines);
    let (mut out, mut err) = (String::new(), String::new());
    let mut output = output;
    for (is_out, line) in received {
        if let Some(output) = output.as_mut() {
            output(&line);
        }
        let buffer = if is_out { &mut out } else { &mut err };
        buffer.push_str(&line);
        buffer.push('\n');
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait()?;
    if !status.success() {
        let code = status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        return fail(format!("{program} failed ({code}):\n{err}\n{out}"));
    }
    Ok(out)
}

/// [`run`] with string arguments.
pub fn run_args(program: &str, arguments: &[&str], directory: Option<&Path>) -> Result<String> {
    let arguments: Vec<String> = arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect();
    run(program, &arguments, directory, None)
}

/// Reads a YAML file whose top is a map, as JSON values.
pub fn read_yaml_map(path: &Path) -> Result<Map> {
    let text = std::fs::read_to_string(path).map_err(|error| Error::file(path, &error))?;
    let value: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml_ng::from_str(&text)
            .map_err(|error| Error::format(format!("{error} in {}", path.display())))?
    };
    match value {
        Value::Object(map) => Ok(map),
        _ => fail(format!("Expected a map in {}", path.display())),
    }
}

/// Reads a JSON file whose top is an object.
pub fn read_json_map(path: &Path) -> Result<Map> {
    let bytes = std::fs::read(path).map_err(|error| Error::file(path, &error))?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => fail(format!("Expected an object in {}", path.display())),
        Err(error) => Err(Error::format(format!("{error} in {}", path.display()))),
    }
}
