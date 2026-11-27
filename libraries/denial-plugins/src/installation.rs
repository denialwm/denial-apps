//! What the packaging says about this installation, in
//! `denial-plugins.installation.json` beside the `denial-plugins` command.
//! Users never configure these paths.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::util::{self, Map, Result, fail, is_word, normalize, read_json_map};

/// The file's name.
pub const FILE: &str = "denial-plugins.installation.json";

/// A Denial checkout whose plugins compile in place, with the Flutter tool
/// and the engine output the checkout's own shell is built with.
#[derive(Clone, Debug, PartialEq)]
pub struct Development {
    /// Holds `packages/`, `plugins/` and `compositor/`.
    pub checkout: PathBuf,
    /// The Flutter launcher, `bin/flutter` of the Flutter source tree.
    pub flutter: PathBuf,
    /// The folder that owns `out/<engine_target>`.
    pub engine_root: PathBuf,
    pub engine_target: String,
    pub platform: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installation {
    /// The packaged release compiler kit; None for a development checkout.
    pub build_kit: Option<PathBuf>,
    pub development: Option<Development>,
    pub denialctl: PathBuf,
    /// The Dart a build kit runs, when the packaging names one; else `dart`
    /// in PATH.
    pub dart: Option<PathBuf>,
    /// Folders searched before PATH for the programs the manager runs, such
    /// as git, when the packaging names them.
    pub path: Vec<PathBuf>,
}

impl Installation {
    /// Reads the metadata in `folder`.
    pub fn read(folder: &Path) -> Result<Self> {
        let file = folder.join(FILE);
        if !file.exists() {
            return fail(
                "Denial plugin tools are incomplete. Update or repair the Denial installation.",
            );
        }
        let value = read_json_map(&file)?;
        if value.get("schema") != Some(&Value::from(1)) {
            return fail("Unsupported Denial plugin installation metadata");
        }
        let resolve = |from: &Map, key: &str| -> Result<PathBuf> {
            match util::text(from, key) {
                Some(path) if !path.is_empty() => Ok(normalize(&folder.join(path))),
                _ => fail(format!("Incomplete Denial plugin installation: {key}")),
            }
        };
        let word = |from: &Map, key: &str| -> Result<String> {
            match util::text(from, key) {
                Some(value) if is_word(value) => Ok(value.to_owned()),
                _ => fail(format!("Incomplete Denial plugin installation: {key}")),
            }
        };
        let dart = match value.get("dart") {
            Some(Value::String(_)) => Some(resolve(&value, "dart")?),
            _ => None,
        };
        let mut path = Vec::new();
        for entry in util::list(&value, "path") {
            match entry.as_str() {
                Some(entry) if !entry.is_empty() => path.push(normalize(&folder.join(entry))),
                _ => return fail("Incomplete Denial plugin installation: path"),
            }
        }
        if let Some(Value::Object(development)) = value.get("development") {
            return Ok(Self {
                build_kit: None,
                development: Some(Development {
                    checkout: resolve(development, "checkout")?,
                    flutter: resolve(development, "flutter")?,
                    engine_root: resolve(development, "engineRoot")?,
                    engine_target: word(development, "engineTarget")?,
                    platform: word(development, "platform")?,
                }),
                denialctl: resolve(&value, "denialctl")?,
                dart,
                path,
            });
        }
        Ok(Self {
            build_kit: Some(resolve(&value, "buildKit")?),
            development: None,
            denialctl: resolve(&value, "denialctl")?,
            dart,
            path,
        })
    }

    /// The `denial-plugins` command of the Denial that runs: the one the
    /// session names, else the one in PATH. None without plugin support.
    pub fn command() -> Option<PathBuf> {
        let named = std::env::var_os("DENIAL_PLUGINS_BINARY")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        named.or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|folder| folder.join("denial-plugins"))
                .find(|path| path.is_file())
        })
    }

    /// The folder holding the metadata beside `command`, its links followed.
    pub fn folder_of(command: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(command).ok()?;
        let folder = real.parent()?.to_path_buf();
        folder.join(FILE).is_file().then_some(folder)
    }

    /// The installation of the Denial that runs, found from its command.
    pub fn locate() -> Result<Self> {
        let folder = Self::command().and_then(|command| Self::folder_of(&command));
        match folder {
            Some(folder) => Self::read(&folder),
            None => fail(
                "Denial plugin tools are incomplete. Update or repair the Denial installation.",
            ),
        }
    }

    /// The installation beside the running executable.
    pub fn beside_executable() -> Result<Self> {
        let executable = std::env::current_exe()?;
        let real = std::fs::canonicalize(&executable).unwrap_or(executable);
        Self::read(real.parent().unwrap_or(Path::new("/")))
    }
}
