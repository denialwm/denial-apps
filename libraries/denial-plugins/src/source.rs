//! Where plugins come from: a Git repository, a folder, or the plugins
//! Denial comes with. Fetching reads repository bytes only; hooks, build
//! scripts and Dart code never run to inspect a plugin. Pub stays the
//! dependency resolver.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::object;
use crate::util::{
    self, Map, Result, canonical, fail, is_commit, is_within, read_yaml_map, run_args, sha256, text,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Git,
    Builtin,
    Local,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Git => "git",
            Kind::Builtin => "builtin",
            Kind::Local => "local",
        }
    }
}

/// What the user asked for, before it is resolved to a revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub kind: Kind,
    pub location: String,
    pub path: String,
    pub reference: Option<String>,
}

impl Source {
    pub fn new(kind: Kind, location: &str, path: &str, reference: Option<&str>) -> Result<Self> {
        if location.is_empty() || location.contains('\0') || location.starts_with('-') {
            return fail("Invalid plugin source");
        }
        if path.starts_with('/') || path.contains('\\') || path.split('/').any(|part| part == "..")
        {
            return fail("Package path must stay inside its repository");
        }
        if let Some(reference) = reference
            && (reference.is_empty() || reference.starts_with('-') || reference.contains('\0'))
        {
            return fail("Invalid Git ref");
        }
        Ok(Self {
            kind,
            location: location.to_owned(),
            path: path.to_owned(),
            reference: reference.map(str::to_owned),
        })
    }

    pub fn from_json(value: &Value) -> Result<Self> {
        let map = util::obj(value);
        let kind = match text(map, "kind") {
            Some("git") => Kind::Git,
            Some("builtin") => Kind::Builtin,
            Some("local") => Kind::Local,
            _ => return fail("Invalid plugin source"),
        };
        Self::new(
            kind,
            text(map, "location").unwrap_or(""),
            text(map, "path").unwrap_or("."),
            text(map, "ref"),
        )
    }

    pub fn to_json(&self) -> Map {
        let mut map = object! {
            "kind" => self.kind.name(),
            "location" => self.location.clone(),
            "path" => self.path.clone(),
        };
        if let Some(reference) = &self.reference {
            map.insert("ref".into(), reference.clone().into());
        }
        map
    }

    pub fn identity(&self) -> String {
        sha256(util::encode(&Value::Object(self.to_json())).as_bytes())
    }
}

/// A source resolved to a package and a revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub source: Source,
    pub name: String,
    pub directory: String,
    pub revision: String,
    pub description: String,
}

impl Resolved {
    pub fn to_json(&self) -> Map {
        object! {
            "source" => self.source.to_json(),
            "name" => self.name.clone(),
            "directory" => self.directory.clone(),
            "revision" => self.revision.clone(),
            "description" => self.description.clone(),
        }
    }
}

/// Whether `name` is a Dart package name.
pub fn is_package_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Git repositories in a cache: one bare mirror per URL and one checkout per
/// revision, each published only once complete.
#[derive(Clone, Debug)]
pub struct Repositories {
    pub cache: PathBuf,
}

impl Repositories {
    pub fn new(cache: impl Into<PathBuf>) -> Self {
        Self {
            cache: cache.into(),
        }
    }

    /// The folder holding `source`'s files, and its revision.
    pub fn checkout(&self, source: &Source, pinned: Option<&str>) -> Result<(PathBuf, String)> {
        if source.kind != Kind::Git {
            let directory = canonical(&util::absolute(Path::new(&source.location)))?;
            return Ok((directory, pinned.unwrap_or("local").to_owned()));
        }
        if let Some(pinned) = pinned
            && !is_commit(pinned)
        {
            return fail("Git resolution must be a full commit identity");
        }
        std::fs::create_dir_all(&self.cache)?;
        let key = sha256(source.location.as_bytes());
        let mirror = self.cache.join(format!("{key}.git"));
        let mirror_path = util::display(&mirror);
        if !mirror.exists() {
            let staging = util::temporary_directory(&self.cache, ".mirror-")?;
            let cloned = run_args(
                "git",
                &[
                    "clone",
                    "--bare",
                    "--",
                    &source.location,
                    &util::display(&staging),
                ],
                None,
            )
            .and_then(|_| Ok(std::fs::rename(&staging, &mirror)?));
            if staging.exists() {
                util::remove_tree(&staging);
            }
            cloned?;
        } else if pinned.is_none() {
            run_args(
                "git",
                &[
                    "-C",
                    &mirror_path,
                    "fetch",
                    "--prune",
                    "origin",
                    "+refs/heads/*:refs/heads/*",
                    "+refs/tags/*:refs/tags/*",
                ],
                None,
            )?;
        }
        let revision = match pinned {
            Some(pinned) => pinned.to_owned(),
            None => {
                let requested = match &source.reference {
                    Some(reference) => reference.clone(),
                    None => {
                        // Ask for the remote HEAD: a bare clone's HEAD goes
                        // stale when upstream changes its default branch.
                        let head = run_args(
                            "git",
                            &["ls-remote", "--symref", "--", &source.location, "HEAD"],
                            None,
                        )?;
                        head.lines()
                            .find_map(|line| {
                                let (commit, name) = line.split_once(char::is_whitespace)?;
                                (name.trim() == "HEAD" && is_commit(commit))
                                    .then(|| commit.to_owned())
                            })
                            .ok_or_else(|| {
                                util::Error::new("Repository has no default-branch commit")
                            })?
                    }
                };
                run_args(
                    "git",
                    &[
                        "-C",
                        &mirror_path,
                        "rev-parse",
                        "--verify",
                        "--end-of-options",
                        &format!("{requested}^{{commit}}"),
                    ],
                    None,
                )?
                .trim()
                .to_owned()
            }
        };
        if !is_commit(&revision) {
            return fail("Git did not return a full commit identity");
        }
        let destination = self.cache.join(format!("{key}-{revision}"));
        if !destination.exists() {
            // Publish only a completed checkout: a failed one must not leave
            // a folder that a later attempt takes as ready.
            let staging = util::temporary_directory(&self.cache, ".checkout-")?;
            let staged = util::display(&staging);
            let checked = run_args(
                "git",
                &["clone", "--no-checkout", "--", &mirror_path, &staged],
                None,
            )
            .and_then(|_| {
                run_args(
                    "git",
                    &[
                        "-C",
                        &staged,
                        "-c",
                        "core.hooksPath=/dev/null",
                        "checkout",
                        "--detach",
                        &revision,
                    ],
                    None,
                )
            })
            .and_then(|_| Ok(std::fs::rename(&staging, &destination)?));
            if staging.exists() {
                util::remove_tree(&staging);
            }
            checked?;
        }
        Ok((destination, revision))
    }

    pub fn resolve(&self, source: &Source, pinned: Option<&str>) -> Result<Resolved> {
        let (repository, revision) = self.checkout(source, pinned)?;
        let directory = canonical(&repository.join(&source.path))?;
        if directory != repository && !is_within(&repository, &directory) {
            return fail("Package symlink escapes its source repository");
        }
        let pubspec = read_yaml_map(&directory.join("pubspec.yaml"))?;
        let name = match text(&pubspec, "name") {
            Some(name) if is_package_name(name) => name.to_owned(),
            _ => return fail("Invalid Dart package name"),
        };
        Ok(Resolved {
            source: source.clone(),
            name,
            directory: util::display(&directory),
            revision,
            description: text(&pubspec, "description").unwrap_or("").to_owned(),
        })
    }

    /// The packages in `source` that look like plugins. Only discovery while
    /// planning confirms one is.
    pub fn inspect(&self, source: &Source) -> Result<Vec<Value>> {
        let (repository, revision) = self.checkout(source, None)?;
        let base = repository.join(&source.path);
        let canonical_base = canonical(&base)?;
        if canonical_base != repository && !is_within(&repository, &canonical_base) {
            return fail("Package path escapes repository");
        }
        let mut found = Vec::new();
        visit(&base, &repository, &revision, &mut found)?;
        found.sort_by(|a: &Value, b: &Value| {
            a["path"]
                .as_str()
                .unwrap_or("")
                .cmp(b["path"].as_str().unwrap_or(""))
        });
        Ok(found)
    }
}

/// Whether a Dart source has something like `@Plugin(` or `@sdk.Plugin(`.
fn mentions_plugin(source: &str) -> bool {
    let bytes = source.as_bytes();
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    for (at, _) in source.match_indices('@') {
        let mut index = at + 1;
        while index < bytes.len() && word(bytes[index]) {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
        }
        if !bytes[index..].starts_with(b"Plugin") {
            // `@Plugin` itself, without the optional prefix.
            let mut start = at + 1;
            while start < bytes.len() && word(bytes[start]) {
                start += 1;
            }
            if !source[at + 1..start].ends_with("Plugin") {
                continue;
            }
            index = start;
        } else {
            index += "Plugin".len();
        }
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'(' {
            return true;
        }
    }
    false
}

fn dart_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(listing) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => dart_files(&path, files),
            Ok(kind) if kind.is_file() && path.extension().is_some_and(|ext| ext == "dart") => {
                files.push(path)
            }
            _ => {}
        }
    }
}

fn visit(
    directory: &Path,
    repository: &Path,
    revision: &str,
    found: &mut Vec<Value>,
) -> Result<()> {
    let manifest = directory.join("pubspec.yaml");
    if manifest.exists() {
        let pubspec = read_yaml_map(&manifest)?;
        let lib = directory.join("lib");
        if lib.is_dir() {
            let mut files = Vec::new();
            dart_files(&lib, &mut files);
            let candidate = files.iter().any(|file| {
                std::fs::read_to_string(file).is_ok_and(|source| mentions_plugin(&source))
            });
            if candidate {
                found.push(Value::Object(object! {
                    "name" => pubspec.get("name").cloned().unwrap_or(Value::Null),
                    "description" => pubspec.get("description").cloned().unwrap_or_else(|| "".into()),
                    "path" => util::display(&util::relative(directory, repository)),
                    "revision" => revision,
                }));
            }
        }
    }
    let mut children: Vec<PathBuf> = std::fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    children.sort();
    for child in children {
        let name = util::name_of(&child);
        if [
            ".git",
            ".dart_tool",
            "build",
            "test",
            "tests",
            "example",
            "examples",
        ]
        .contains(&name.as_str())
        {
            continue;
        }
        visit(&child, repository, revision, found)?;
    }
    Ok(())
}
