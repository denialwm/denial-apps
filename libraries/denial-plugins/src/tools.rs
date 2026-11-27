//! What a selection compiles with: the Denial source it is built for, a
//! Flutter tool and the engine's compilers. A package carries them as a
//! build kit, which is copied once into the user's state; a development
//! checkout uses its own, in place.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::available::{Available, catalog_configuration};
use crate::installation::{Development, Installation};
use crate::object;
use crate::source::Repositories;
use crate::store::Store;
use crate::util::{
    self, Map, Result, canonical, content_key, display, fail, file_digest, is_within, is_word,
    normalize, obj, read_json_map, sha256, text,
};
use crate::version;

/// What a composition built from `checkout` is stamped with: the checkout's
/// path alone, so commits, edits and engine rebuilds never refuse plugins.
pub fn development_identity(checkout: &Path) -> Value {
    let real = std::fs::canonicalize(checkout).unwrap_or_else(|_| checkout.to_path_buf());
    Value::Object(object! {"schema" => 1, "development" => display(&real)})
}

/// The identity of `runtime`: a release's marker, or a development checkout.
pub fn source_identity(runtime: &Path) -> Result<Value> {
    let marker = runtime.join(".denial-ui-source.json");
    if marker.exists() {
        Ok(Value::Object(read_json_map(&marker)?))
    } else {
        Ok(development_identity(runtime))
    }
}

/// Built-in plugins are selected once, for a selection nobody touched yet:
/// removing every plugin on purpose survives restarts and upgrades.
fn seed_defaults(store: &Store, repositories: &Repositories, runtime: &str) -> Result<()> {
    if store.read("initialization.json")?.get("complete") == Some(&Value::Bool(true)) {
        return Ok(());
    }
    let selection = store.selection()?;
    if selection.get("revision").and_then(Value::as_i64) == Some(0)
        && obj(&selection["roots"]).is_empty()
    {
        Available {
            store,
            repositories,
        }
        .defaults(Some(runtime))?;
    }
    store.write("initialization.json", object! {"complete" => true})
}

/// A Dart SDK that matched the version a kit asks for.
struct DartSdk {
    root: PathBuf,
    executable: PathBuf,
    version: String,
}

/// Why no usable Dart was found.
struct DartProblem {
    message: String,
    found: bool,
    version: Option<String>,
}

fn find_dart(
    expected: &str,
    constraint: &str,
    executable: Option<&Path>,
) -> std::result::Result<DartSdk, DartProblem> {
    let missing = || DartProblem {
        message: format!(
            "Dart {expected} was not found in PATH. Install the dart package and try again."
        ),
        found: false,
        version: None,
    };
    let command = match executable {
        Some(path) => path.to_path_buf(),
        None => std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(|folder| util::absolute(&folder.join("dart")))
            .find(|path| path.exists())
            .ok_or_else(missing)?,
    };
    if !command.exists() {
        return Err(missing());
    }
    let resolved = std::fs::canonicalize(&command).map_err(|_| DartProblem {
        message: format!("The dart command in PATH could not be resolved. Reinstall Dart {expected} and try again."),
        found: true,
        version: None,
    })?;
    let mode = std::fs::metadata(&resolved)
        .map(|stat| stat.permissions().mode())
        .unwrap_or(0);
    if mode & 0o111 == 0 {
        return Err(DartProblem {
            message: "The dart command in PATH is not executable. Reinstall the dart package and try again.".into(),
            found: true,
            version: None,
        });
    }
    let grand = |path: &Path| path.parent().and_then(Path::parent).map(Path::to_path_buf);
    let mut roots = Vec::new();
    for root in [grand(&resolved), grand(&util::absolute(&command))]
        .into_iter()
        .flatten()
    {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    for root in roots {
        let Ok(found) = std::fs::read_to_string(root.join("version")) else {
            continue;
        };
        let found = found.trim().to_owned();
        let compatible = version::parse_version(&found)
            .and_then(|parsed| version::allows(constraint, &parsed))
            .map_err(|_| DartProblem {
                message: format!("The installed Dart SDK has an invalid version: {found}."),
                found: true,
                version: Some(found.clone()),
            })?;
        if !compatible {
            return Err(DartProblem {
                message: format!(
                    "Dart {found} is installed, but Denial requires {constraint}. Install a compatible dart package and try again."
                ),
                found: true,
                version: Some(found),
            });
        }
        for required in [
            "bin/dart",
            "bin/snapshots/frontend_server_aot.dart.snapshot",
            "lib/core/core.dart",
        ] {
            if !root.join(required).exists() {
                return Err(DartProblem {
                    message: format!(
                        "The Dart {expected} SDK in PATH is incomplete. Reinstall the dart package and try again."
                    ),
                    found: true,
                    version: Some(found),
                });
            }
        }
        return Ok(DartSdk {
            root,
            executable: resolved,
            version: found,
        });
    }
    Err(DartProblem {
        message: format!(
            "The dart command in PATH is not part of a complete Dart SDK. Install Dart {expected} and try again."
        ),
        found: true,
        version: None,
    })
}

fn dart_status(expected: &str, constraint: &str, executable: Option<&Path>) -> Map {
    match find_dart(expected, constraint, executable) {
        Ok(sdk) => object! {
            "available" => true,
            "found" => true,
            "expectedVersion" => expected,
            "constraint" => constraint,
            "version" => sdk.version,
            "executable" => display(&sdk.executable),
        },
        Err(problem) => {
            let mut status = object! {
                "available" => false,
                "found" => problem.found,
                "expectedVersion" => expected,
                "constraint" => constraint,
            };
            if let Some(version) = problem.version {
                status.insert("version".into(), version.into());
            }
            status.insert("error".into(), problem.message.into());
            status
        }
    }
}

pub enum Tools {
    Development(DevelopmentTools),
    Kit(Kit),
}

impl Tools {
    pub fn of(store: &Store, installation: &Installation) -> Self {
        match &installation.development {
            Some(development) => Tools::Development(DevelopmentTools {
                store: store.clone(),
                installation: development.clone(),
            }),
            None => Tools::Kit(Kit {
                store: store.clone(),
                source: installation.build_kit.clone().unwrap_or_default(),
                dart: installation.dart.clone(),
            }),
        }
    }

    /// The installation carries build tools at all.
    pub fn available(&self) -> bool {
        match self {
            Tools::Development(_) => true,
            Tools::Kit(kit) => kit.available(),
        }
    }

    /// Nothing has to be prepared before compiling.
    pub fn ready(&self) -> bool {
        match self {
            Tools::Development(tools) => tools.ready(),
            Tools::Kit(kit) => kit.ready(),
        }
    }

    /// Why nothing can be built here, with what fixes it.
    pub fn problem(&self) -> Option<String> {
        match self {
            Tools::Development(tools) => tools.problem(),
            Tools::Kit(kit) => kit.problem(),
        }
    }

    /// Changes when a failed rebuild is worth trying again.
    pub fn identity(&self) -> Result<String> {
        match self {
            Tools::Development(tools) => Ok(tools.identity()),
            Tools::Kit(kit) => kit.identity(),
        }
    }

    pub fn dart_status(&self) -> Map {
        match self {
            Tools::Development(tools) => tools.dart_status(),
            Tools::Kit(kit) => kit.dart_status(),
        }
    }

    /// Where the tools are, without preparing anything.
    pub fn configuration(&self) -> Map {
        match self {
            Tools::Development(tools) => tools.configuration(),
            Tools::Kit(kit) => {
                catalog_configuration(&kit.store.read("configuration.json").unwrap_or_default())
            }
        }
    }

    /// Prepares the tools. Call under the manager lock.
    pub fn initialize(&self, progress: &dyn Fn(&str)) -> Result<Map> {
        match self {
            Tools::Development(tools) => tools.initialize(),
            Tools::Kit(kit) => kit.initialize(progress),
        }
    }
}

fn repositories(store: &Store) -> Repositories {
    Repositories::new(store.file("repositories"))
}

pub struct DevelopmentTools {
    store: Store,
    installation: Development,
}

impl DevelopmentTools {
    fn dart_sdk(&self) -> PathBuf {
        self.installation
            .flutter
            .parent()
            .unwrap_or(Path::new("/"))
            .join("cache/dart-sdk")
    }

    fn compiler(&self) -> PathBuf {
        self.installation
            .engine_root
            .join("out")
            .join(&self.installation.engine_target)
            .join("gen_snapshot")
    }

    /// What is missing before this checkout can compile plugins.
    fn missing(&self) -> Option<PathBuf> {
        let checkout = &self.installation.checkout;
        [
            checkout.join("plugins/builtins.yaml"),
            checkout.join("packages/denial_flutter_sdk/pubspec.yaml"),
            self.installation.flutter.clone(),
            self.dart_sdk().join("bin/dart"),
            self.compiler(),
        ]
        .into_iter()
        .find(|path| !path.is_file())
    }

    /// The built-in plugin records still point at another installation,
    /// such as the packaged one this machine also has.
    fn foreign_builtins(&self) -> bool {
        let Ok(record) = self.store.read("installed.json") else {
            return true;
        };
        let Some(plugins) = record.get("plugins").and_then(Value::as_object) else {
            return true;
        };
        let own = display(&self.installation.checkout.join("plugins"));
        plugins.values().any(|plugin| {
            plugin["source"]["kind"] == "builtin" && plugin["source"]["location"] != own.as_str()
        })
    }

    fn ready(&self) -> bool {
        self.missing().is_none() && !self.foreign_builtins()
    }

    fn problem(&self) -> Option<String> {
        self.missing().map(|path| {
            format!(
                "This checkout cannot build plugins yet: {} is missing. Build the shell once with tools/denial-pc bundle.",
                path.display()
            )
        })
    }

    fn identity(&self) -> String {
        use std::os::unix::fs::MetadataExt;
        let compiler = std::fs::metadata(self.compiler()).ok().map(|stat| {
            format!(
                "{}:{}",
                stat.size(),
                stat.mtime() * 1_000_000 + stat.mtime_nsec() / 1000
            )
        });
        content_key(&Value::Object(object! {
            "development" => display(&self.installation.checkout),
            "compiler" => compiler,
        }))
    }

    fn dart_status(&self) -> Map {
        let version = self.dart_sdk().join("version");
        match std::fs::read_to_string(&version) {
            Err(_) => object! {
                "available" => false,
                "found" => false,
                "error" => format!(
                    "The Flutter checkout has no Dart SDK yet: {}.",
                    version.parent().unwrap_or(Path::new("/")).display()
                ),
            },
            Ok(found) => object! {
                "available" => true,
                "found" => true,
                "expectedVersion" => found.trim(),
                "version" => found.trim(),
                "executable" => display(&self.dart_sdk().join("bin/dart")),
            },
        }
    }

    fn configuration(&self) -> Map {
        let saved = self.store.read("configuration.json").unwrap_or_default();
        let mut configuration = catalog_configuration(
            &saved
                .into_iter()
                .filter(|(key, _)| key.starts_with("catalog-"))
                .collect(),
        );
        let root = display(&self.installation.engine_root);
        let target = &self.installation.engine_target;
        configuration.extend(object! {
            "schema" => 1,
            "development" => true,
            "runtime" => display(&self.installation.checkout),
            "flutter" => display(&self.installation.flutter),
            // The Flutter tool takes Denial's dart:ui from the engine output
            // for every command, as the kit's launcher does.
            "flutter-arguments" => vec![
                format!("--local-engine-src-path={root}"),
                format!("--local-engine={target}"),
                format!("--local-engine-host={target}"),
            ],
            "engine-root" => root.clone(),
            "engine-target" => target.clone(),
            "platform" => self.installation.platform.clone(),
            "dart-sdk" => display(&self.dart_sdk()),
        });
        configuration
    }

    fn initialize(&self) -> Result<Map> {
        if let Some(problem) = self.problem() {
            return fail(problem);
        }
        let repositories = repositories(&self.store);
        let checkout = display(&self.installation.checkout);
        seed_defaults(&self.store, &repositories, &checkout)?;
        Available {
            store: &self.store,
            repositories: &repositories,
        }
        .adopt_builtins(&checkout)?;
        Ok(self.configuration())
    }
}

/// The packaged release compiler, copied into a writable per-user cache:
/// Flutter writes locks and stamps, and the packaged files stay untouched.
pub struct Kit {
    pub store: Store,
    pub source: PathBuf,
    pub dart: Option<PathBuf>,
}

impl Kit {
    fn available(&self) -> bool {
        self.source.join("kit.json").is_file()
    }

    fn manifest(&self) -> Option<Map> {
        read_json_map(&self.source.join("kit.json")).ok()
    }

    fn problem(&self) -> Option<String> {
        if !self.available() {
            return Some(
                "Denial plugin build tools are missing. Install the denial-plugin-manager package."
                    .into(),
            );
        }
        text(&self.dart_status(), "error").map(str::to_owned)
    }

    fn identity(&self) -> Result<String> {
        match std::fs::read(self.source.join("kit.json")) {
            Ok(bytes) => Ok(sha256(&bytes)),
            Err(_) => fail("Denial plugin build tools are missing. Update or repair Denial."),
        }
    }

    fn versions(&self) -> Option<(String, String)> {
        let manifest = self.manifest()?;
        let expected = text(&manifest, "dartVersion").filter(|value| !value.is_empty())?;
        let constraint = text(&manifest, "dartConstraint").filter(|value| !value.is_empty())?;
        Some((expected.to_owned(), constraint.to_owned()))
    }

    fn dart_status(&self) -> Map {
        match self.versions() {
            Some((expected, constraint)) => {
                dart_status(&expected, &constraint, self.dart.as_deref())
            }
            None => object! {
                "available" => false,
                "found" => false,
                "error" => "The installed plugin build kit does not declare its Dart version.",
            },
        }
    }

    fn ready(&self) -> bool {
        if !self.available() {
            return false;
        }
        let (Ok(identity), Some((expected, constraint))) = (self.identity(), self.versions())
        else {
            return false;
        };
        let Ok(dart) = find_dart(&expected, &constraint, self.dart.as_deref()) else {
            return false;
        };
        let config = self.store.read("configuration.json").unwrap_or_default();
        let root = self.store.root.join("build-kits").join(&identity);
        let target = text(&config, "engine-target").unwrap_or("");
        let is = |key: &str, path: &Path| text(&config, key) == Some(display(path).as_str());
        text(&config, "build-kit") == Some(identity.as_str())
            && is("runtime", &root.join("runtime"))
            && is("flutter", &root.join("flutter/bin/flutter"))
            && is("engine-root", &root.join("engine"))
            && is("dart-sdk", &dart.root)
            && is("dart", &dart.executable)
            && text(&config, "dart-version") == Some(dart.version.as_str())
            && text(&config, "dart-constraint") == Some(constraint.as_str())
            && root.join("runtime/.denial-ui-source.json").is_file()
            && root.join("flutter/bin/flutter").is_file()
            && links_to(&root.join("flutter/bin/cache/dart-sdk"), &dart.root)
            && links_to(
                &root.join("engine/out").join(target).join("dart-sdk"),
                &dart.root,
            )
            && root
                .join("engine/out")
                .join(target)
                .join("gen_snapshot")
                .is_file()
    }

    /// Defaults are seeded once: removing every plugin on purpose survives
    /// restarts. Call under the manager lock.
    fn initialize(&self, progress: &dyn Fn(&str)) -> Result<Map> {
        let configuration = if self.ready() {
            let configuration = catalog_configuration(&self.store.read("configuration.json")?);
            self.store
                .write("configuration.json", configuration.clone())?;
            configuration
        } else {
            self.prepare(progress)?
        };
        seed_defaults(
            &self.store,
            &repositories(&self.store),
            text(&configuration, "runtime").unwrap_or(""),
        )?;
        Ok(configuration)
    }

    fn prepare(&self, progress: &dyn Fn(&str)) -> Result<Map> {
        if !self.available() {
            return fail(
                "Installed plugin build tools are missing. Update or repair the Denial installation.",
            );
        }
        let bytes = std::fs::read(self.source.join("kit.json"))?;
        let manifest = match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(manifest)) => manifest,
            _ => return fail("Unsupported plugin build kit"),
        };
        let platform = text(&manifest, "platform").unwrap_or("");
        let expected = text(&manifest, "dartVersion").unwrap_or("");
        let constraint = text(&manifest, "dartConstraint").unwrap_or("");
        let target = text(&manifest, "engineTarget").unwrap_or("");
        if manifest.get("schema").and_then(Value::as_i64) != Some(1)
            || !["linux-x64", "linux-arm64"].contains(&platform)
            || expected.is_empty()
            || constraint.is_empty()
            || !is_word(target)
        {
            return fail("Unsupported plugin build kit");
        }
        let id = sha256(&bytes);
        let destination = self.store.root.join("build-kits").join(&id);
        let dart = find_dart(expected, constraint, self.dart.as_deref())
            .map_err(|problem| util::Error::new(problem.message))?;
        progress("Verifying the installed release build tools");
        verify(&self.source, &manifest)?;
        if !destination.exists() {
            let parent = destination.parent().unwrap_or(Path::new("/"));
            let temporary = util::temporary_directory(parent, "preparing-")?;
            let prepared = (|| -> Result<()> {
                progress("Preparing your compiler cache");
                util::run(
                    "cp",
                    &[
                        "-a".into(),
                        "--".into(),
                        format!("{}/.", display(&self.source)),
                        display(&temporary),
                    ],
                    None,
                    None,
                )?;
                verify(&temporary, &manifest)?;
                // Nobody uses this folder before it is complete.
                util::run_args("chmod", &["-R", "u+rwX", "--", &display(&temporary)], None)?;
                std::fs::rename(&temporary, &destination)?;
                Ok(())
            })();
            if temporary.exists() {
                util::remove_tree(&temporary);
            }
            prepared?;
        } else {
            verify(&destination, &manifest)?;
        }
        link_dart_sdk(&destination, target, &dart.root)?;
        let saved = self.store.read("configuration.json")?;
        let mut configuration = catalog_configuration(
            &saved
                .into_iter()
                .filter(|(key, _)| key.starts_with("catalog-"))
                .collect(),
        );
        configuration.extend(object! {
            "schema" => 1,
            "runtime" => display(&destination.join("runtime")),
            "flutter" => display(&destination.join("flutter/bin/flutter")),
            "engine-root" => display(&destination.join("engine")),
            "engine-target" => target,
            "dart-sdk" => display(&dart.root),
            "dart" => display(&dart.executable),
            "dart-version" => dart.version.clone(),
            "dart-constraint" => constraint,
            "platform" => platform,
            "build-kit" => id.clone(),
        });
        let repositories = repositories(&self.store);
        Available {
            store: &self.store,
            repositories: &repositories,
        }
        .adopt_builtins(text(&configuration, "runtime").unwrap_or(""))?;
        self.store
            .write("configuration.json", configuration.clone())?;
        prune_stale_kits(&destination);
        Ok(configuration)
    }
}

fn prune_stale_kits(kept: &Path) {
    let Some(parent) = kept.parent() else { return };
    let Ok(listing) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        let name = util::name_of(&path);
        let identity = name.len() == 64
            && name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if path != kept && identity && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            // A stale cache must not stop the newly verified kit.
            util::remove_tree(&path);
        }
    }
}

/// Checks every file and link of a kit against its manifest.
fn verify(root: &Path, manifest: &Map) -> Result<()> {
    let base = util::absolute(root);
    let real_base = canonical(&base)?;
    let checked = |relative: &str| -> Result<PathBuf> {
        let path = base.join(relative);
        if relative.starts_with('/')
            || display(&normalize(Path::new(relative))) != relative
            || !is_within(&base, &path)
        {
            return fail(format!("Invalid build kit path: {relative}"));
        }
        // A parent link must not lead verification out of the kit.
        if !is_within(&real_base, &canonical(&path)?) {
            return fail(format!("Build kit path escapes its root: {relative}"));
        }
        Ok(path)
    };
    let files = obj(manifest.get("files").unwrap_or(&Value::Null));
    for (relative, digest) in files {
        let path = checked(relative)?;
        let link = std::fs::symlink_metadata(&path).is_ok_and(|stat| stat.is_symlink());
        if link || file_digest(&path).ok().as_deref() != digest.as_str() {
            return fail(format!("Build kit input changed: {relative}"));
        }
    }
    for (relative, target) in obj(manifest.get("links").unwrap_or(&Value::Null)) {
        let path = checked(relative)?;
        let actual = std::fs::read_link(&path)
            .ok()
            .map(|target| display(&target));
        if actual.as_deref() != target.as_str() {
            return fail(format!("Build kit link changed: {relative}"));
        }
    }
    let target = text(manifest, "engineTarget").unwrap_or("");
    for required in [
        "runtime/.denial-ui-source.json".to_owned(),
        "runtime/plugins/builtins.yaml".to_owned(),
        "flutter/bin/flutter".to_owned(),
        format!("engine/out/{target}/gen_snapshot"),
    ] {
        if !files.contains_key(&required) {
            return fail(format!("Incomplete build kit: {required}"));
        }
    }
    Ok(())
}

fn links_to(path: &Path, target: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|stat| stat.is_symlink())
        && match (std::fs::canonicalize(path), std::fs::canonicalize(target)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

fn link_dart_sdk(destination: &Path, target: &str, dart_root: &Path) -> Result<()> {
    for path in [
        destination.join("flutter/bin/cache/dart-sdk"),
        destination.join("engine/out").join(target).join("dart-sdk"),
    ] {
        match std::fs::symlink_metadata(&path) {
            Ok(stat) if stat.is_symlink() => std::fs::remove_file(&path)?,
            Ok(_) => {
                return fail(format!(
                    "Plugin build kit path is not a link: {}",
                    path.display()
                ));
            }
            Err(_) => {}
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(dart_root, &path)?;
    }
    Ok(())
}
