//! Planning: a fresh candidate workspace with snapshots of the SDK and the
//! local plugins, Pub's resolution, discovery and the generated shell. A
//! plan is never edited in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::available::Available;
use crate::boundary;
use crate::discovery::{Discovery, Runner, TypeId};
use crate::generator::Plan;
use crate::graph::Graph;
use crate::object;
use crate::source::{Kind, Repositories, Source};
use crate::store::{DigestCache, Store, validate_id};
use crate::tools::source_identity;
use crate::util::{
    self, Error, Map, Result, canonical, content_key, display, encode, fail, obj, pretty,
    read_json_map, read_yaml_map, relative, text,
};
use crate::version;

/// The contract the application itself needs a provider of.
pub fn shell_application() -> TypeId {
    TypeId {
        library: "package:denial_flutter_sdk/application.dart".into(),
        name: "ShellApplication".into(),
    }
}

/// Folders and files never copied into a snapshot.
const DISPOSABLE: [&str; 6] = [
    ".git",
    ".dart_tool",
    "build",
    ".flutter-plugins",
    ".flutter-plugins-dependencies",
    "pubspec_overrides.yaml",
];

/// Copies a source tree without generated output, refusing links: a link
/// would read mutable inputs from outside.
pub fn copy_source_tree(source: &Path, destination: &Path, source_root: &Path) -> Result<()> {
    std::fs::create_dir_all(destination).map_err(|error| Error::file(destination, &error))?;
    for entry in std::fs::read_dir(source).map_err(|error| Error::file(source, &error))? {
        let entry = entry?;
        let path = entry.path();
        let name = util::name_of(&path);
        let relative = path.strip_prefix(source_root).unwrap_or(&path);
        if [
            "linux/flutter/ephemeral",
            "windows/flutter/ephemeral",
            "macos/Flutter/ephemeral",
        ]
        .iter()
        .any(|skipped| relative == Path::new(skipped))
            || DISPOSABLE.contains(&name.as_str())
        {
            continue;
        }
        let target = destination.join(&name);
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return fail(format!(
                "Source snapshot contains a symlink: {}",
                path.display()
            ));
        } else if kind.is_dir() {
            copy_source_tree(&path, &target, source_root)?;
        } else if kind.is_file() {
            std::fs::copy(&path, &target).map_err(|error| Error::file(&path, &error))?;
        }
    }
    Ok(())
}

/// Path packages copied into a candidate, each once, with dependencies
/// rewritten to the copies. Dependencies come from pubspecs only.
pub struct Snapshot {
    destination: PathBuf,
    overrides: BTreeMap<String, PathBuf>,
    originals: BTreeMap<String, PathBuf>,
    copies: BTreeMap<String, PathBuf>,
}

impl Snapshot {
    pub fn new(destination: PathBuf, overrides: BTreeMap<String, PathBuf>) -> Self {
        Self {
            destination,
            overrides,
            originals: BTreeMap::new(),
            copies: BTreeMap::new(),
        }
    }

    pub fn copy(&mut self, source: &Path) -> Result<PathBuf> {
        let real = canonical(source)?;
        let mut manifest = read_yaml_map(&real.join("pubspec.yaml"))?;
        let name = text(&manifest, "name").unwrap_or("").to_owned();
        if let Some(replacement) = self.overrides.get(&name).cloned()
            && canonical(&replacement)? != real
        {
            return self.copy(&replacement);
        }
        if let Some(original) = self.originals.get(&name) {
            if *original != real {
                return fail(format!(
                    "Package {name} has conflicting local sources: {} and {}",
                    original.display(),
                    real.display()
                ));
            }
            return Ok(self.copies[&name].clone());
        }
        let target = self.destination.join(&name);
        self.originals.insert(name.clone(), real.clone());
        self.copies.insert(name.clone(), target.clone());
        copy_source_tree(&real, &target, &real)?;
        // Installed sources can be read-only; this private copy is
        // rewritten and receives generated output.
        util::run_args("chmod", &["-R", "u+rwX", "--", &display(&target)], None).map_err(
            |error| Error::new(format!("Cannot prepare writable snapshot {name}: {error}")),
        )?;
        for key in [
            "dev_dependencies",
            "dependency_overrides",
            "workspace",
            "resolution",
        ] {
            manifest.shift_remove(key);
        }
        let mut dependencies = obj(manifest.get("dependencies").unwrap_or(&Value::Null)).clone();
        for (dependency, value) in dependencies.clone() {
            let Some(path) = value.get("path").and_then(Value::as_str) else {
                continue;
            };
            let from = self
                .overrides
                .get(&dependency)
                .cloned()
                .unwrap_or_else(|| util::normalize(&real.join(path)));
            let copied = self.copy(&from)?;
            let mut rewritten = object! {"path" => display(&relative(&copied, &target))};
            if let Some(version) = value.get("version").filter(|version| !version.is_null()) {
                rewritten.insert("version".into(), version.clone());
            }
            dependencies.insert(dependency, Value::Object(rewritten));
        }
        manifest.insert("dependencies".into(), Value::Object(dependencies));
        std::fs::write(
            target.join("pubspec.yaml"),
            pretty(&Value::Object(manifest)),
        )?;
        // A copy never inherits a developer's package configuration, lock or
        // overrides.
        for name in ["pubspec_overrides.yaml", "pubspec.lock"] {
            let _ = std::fs::remove_file(target.join(name));
        }
        Ok(target)
    }
}

/// Whether the saved `selection` asks for the composition `plan` was built
/// from. Built-in plugins move with every release, so their names are the
/// intent; Git and local plugins must keep their source. Resolved Git
/// revisions are plan state, never intent.
pub fn same_composition(selection: &Map, plan: &Map) -> bool {
    let saved = obj(selection.get("roots").unwrap_or(&Value::Null));
    let built = obj(plan.get("roots").unwrap_or(&Value::Null));
    if saved.len() != built.len() {
        return false;
    }
    for (name, value) in saved {
        let Some(used) = built.get(name) else {
            return false;
        };
        let wanted = &value["source"];
        let used = &used["source"];
        if wanted["kind"] == "builtin" && used["kind"] == "builtin" {
            continue;
        }
        let as_map = |value: &Value| Value::Object(obj(value).clone());
        if content_key(&as_map(wanted)) != content_key(&as_map(used)) {
            return false;
        }
    }
    let part =
        |map: &Map, key: &str| Value::Object(obj(map.get(key).unwrap_or(&Value::Null)).clone());
    content_key(&part(selection, "selections")) == content_key(&part(plan, "selections"))
        && content_key(&part(selection, "ordering")) == content_key(&part(plan, "ordering"))
}

pub struct Planner<'a> {
    pub store: &'a Store,
    pub repositories: &'a Repositories,
}

pub struct PlanRequest<'a> {
    /// The installed source template, or a development checkout read as it
    /// is now.
    pub runtime: &'a Path,
    pub flutter: &'a Path,
    /// Given before every Flutter command.
    pub flutter_arguments: &'a [String],
    pub candidate: &'a str,
    pub update: bool,
    /// Plans that earlier candidate's composition, not the saved selection,
    /// for the installed release: its roots, provider choices, Pub lock and
    /// Git revisions. It never fetches newer revisions.
    pub rebuild_of: Option<&'a str>,
    /// `DENIAL_SDK_PATH`: another folder holding the two SDK packages.
    pub sdk_root: Option<&'a Path>,
}

impl Planner<'_> {
    pub fn plan(&self, request: &PlanRequest, progress: &dyn Fn(&str)) -> Result<Map> {
        validate_id(request.candidate)?;
        if let Some(base) = request.rebuild_of {
            validate_id(base)?;
            if request.update {
                return fail(
                    "A rebuild restores the confirmed plugin revisions; update separately",
                );
            }
        }
        let store = self.store;
        let candidate = store.root.join("candidates").join(request.candidate);
        if candidate.exists() {
            return fail("Candidate already exists; plans are never edited in place");
        }
        let selection = store.selection()?;
        let base = match request.rebuild_of {
            Some(base) => Some(store.read(&format!("candidates/{base}/plan.json"))?),
            None => None,
        };
        if base
            .as_ref()
            .is_some_and(|base| !base.get("roots").is_some_and(Value::is_object))
        {
            return fail(
                "The plugins that were running can no longer be rebuilt. Open Plugins and apply your selection again.",
            );
        }
        std::fs::create_dir_all(&candidate)?;
        // What to compose: the saved selection, or the confirmed composition
        // a Denial update left behind.
        let intent = base.as_ref().unwrap_or(&selection);
        let roots = obj(intent.get("roots").unwrap_or(&Value::Null)).clone();
        let selections = obj(intent.get("selections").unwrap_or(&Value::Null)).clone();
        let ordering = obj(intent.get("ordering").unwrap_or(&Value::Null)).clone();
        let runtime_text = display(request.runtime);
        let mut builtins = BTreeMap::new();
        for entry in (Available {
            store,
            repositories: self.repositories,
        })
        .builtins(Some(&runtime_text))?
        {
            builtins.insert(
                text(&entry, "name").unwrap_or("").to_owned(),
                Source::from_json(&entry["source"])?,
            );
        }
        let previous_plan = match request.rebuild_of {
            Some(base) => object! {"id" => base},
            None => store.read("last-plan.json")?,
        };
        let previous_metadata = match text(&previous_plan, "id") {
            Some(id) => store.read(&format!("candidates/{id}/plan.json"))?,
            None => Map::new(),
        };
        let previous_roots = obj(previous_metadata.get("roots").unwrap_or(&Value::Null)).clone();
        let sdk_folder = request
            .sdk_root
            .map(Path::to_path_buf)
            .unwrap_or_else(|| request.runtime.join("packages"));
        let mut sdk_sources = BTreeMap::new();
        for name in ["denial_sdk", "denial_flutter_sdk"] {
            let folder = sdk_folder.join(name);
            let manifest = folder.join("pubspec.yaml");
            if !manifest.exists() || text(&read_yaml_map(&manifest)?, "name") != Some(name) {
                return fail(format!(
                    "SDK source must contain denial_sdk and denial_flutter_sdk packages: {}",
                    request.sdk_root.unwrap_or(request.runtime).display()
                ));
            }
            sdk_sources.insert(name.to_owned(), folder);
        }
        let mut snapshot = Snapshot::new(candidate.join("packages"), sdk_sources.clone());
        progress("Snapshotting the matching runtime and public SDKs");
        let sdk = snapshot.copy(&sdk_sources["denial_sdk"])?;
        let flutter_sdk = snapshot.copy(&sdk_sources["denial_flutter_sdk"])?;
        let app = candidate.join("app");
        std::fs::create_dir_all(&app)?;
        let path_to =
            |target: &Path| Value::Object(object! {"path" => display(&relative(target, &app))});
        let mut dependencies = object! {
            "flutter" => object! {"sdk" => "flutter"},
            "denial_sdk" => path_to(&sdk),
            "denial_flutter_sdk" => path_to(&flutter_sdk),
        };
        let mut resolved_roots = Map::new();
        for (name, previous) in &roots {
            let mut source = Source::from_json(&previous["source"])?;
            if source.kind == Kind::Builtin {
                // A built-in comes back by name, from the installed release.
                source = match builtins.get(name) {
                    Some(installed) => installed.clone(),
                    None => {
                        return fail(format!(
                            "{name} is not provided by this installed runtime; remove it or choose another plugin"
                        ));
                    }
                };
            }
            let retained = match previous_roots.get(name) {
                Some(prior) if encode(&prior["source"]) == encode(&previous["source"]) => {
                    prior["revision"].as_str().map(str::to_owned)
                }
                _ => previous["revision"].as_str().map(str::to_owned),
            };
            progress(&format!("Resolving {name}"));
            let pinned = if request.update || source.kind != Kind::Git {
                None
            } else {
                retained.as_deref()
            };
            let resolved = self.repositories.resolve(&source, pinned)?;
            if &resolved.name != name {
                return fail(format!(
                    "{name} now declares {}; select it explicitly as a new package",
                    resolved.name
                ));
            }
            resolved_roots.insert(name.clone(), Value::Object(resolved.to_json()));
            if source.kind == Kind::Git {
                dependencies.insert(
                    name.clone(),
                    Value::Object(object! {"git" => object! {
                        "url" => source.location.clone(),
                        "path" => source.path.clone(),
                        "ref" => resolved.revision.clone(),
                    }}),
                );
            } else {
                let copied = snapshot.copy(Path::new(&resolved.directory))?;
                dependencies.insert(name.clone(), path_to(&copied));
            }
        }
        let sdk_manifest = read_yaml_map(&flutter_sdk.join("pubspec.yaml"))?;
        let manifest = object! {
            "name" => "denial_generated_shell",
            "publish_to" => "none",
            "version" => "0.0.0",
            "environment" => sdk_manifest.get("environment").cloned().unwrap_or(Value::Null),
            "dependencies" => dependencies,
            "flutter" => object! {"uses-material-design" => true},
            "dependency_overrides" => object! {
                "denial_sdk" => path_to(&sdk),
                "denial_flutter_sdk" => path_to(&flutter_sdk),
            },
        };
        std::fs::write(
            app.join("pubspec.yaml"),
            pretty(&Value::Object(manifest.clone())),
        )?;
        if !request.update
            && let Some(id) = text(&previous_plan, "id")
        {
            let old = store.file(&format!("candidates/{id}/app/pubspec.lock"));
            if old.exists() {
                std::fs::copy(&old, app.join("pubspec.lock"))?;
            }
        }
        progress("Resolving packages with Pub");
        let flutter = display(request.flutter);
        let mut arguments = request.flutter_arguments.to_vec();
        arguments.push("pub".into());
        arguments.push(if request.update { "upgrade" } else { "get" }.into());
        util::run(&flutter, &arguments, Some(&app), None)?;
        let graph = Graph::read(&app)?;
        boundary::validate(&graph)?;
        validate_sdk_constraints(
            &graph,
            &[("denial_sdk", &sdk), ("denial_flutter_sdk", &flutter_sdk)],
        )?;
        // Each resolved package is read once for planning, discovery reuse
        // and build provenance. A new workspace path is not new input.
        let mut digests = DigestCache::new(store);
        let mut package_digests = Map::new();
        for package in &graph.packages {
            if package.name == graph.application {
                continue;
            }
            let digest = if util::is_within(&candidate, &package.root) {
                util::plain_tree_digest(&package.root)?
            } else {
                digests.tree(&package.root, &[])?
            };
            package_digests.insert(package.name.clone(), digest.into());
        }
        let configuration = read_json_map(&app.join(".dart_tool/package_config.json"))?;
        let mut languages = Map::new();
        for item in util::list(&configuration, "packages") {
            if let Some(name) = item["name"].as_str()
                && graph.package(name).is_some()
            {
                languages.insert(
                        name.to_owned(),
                        Value::Object(object! {
                            "languageVersion" => item.get("languageVersion").cloned().unwrap_or(Value::Null),
                            "packageUri" => item.get("packageUri").cloned().unwrap_or(Value::Null),
                        }),
                    );
            }
        }
        let runner = Runner::new(request.runtime, request.flutter, store.file("cache"))?;
        let sdk_path = runner.sdk();
        let discovery_key = content_key(&Value::Object(object! {
            "format" => 4,
            "script" => runner.identity()?,
            "packages" => package_digests,
            "languages" => languages,
            "chains" => graph.chains.iter().map(|(name, chain)| (name.clone(), Value::from(chain.clone()))).collect::<Map>(),
            "sdkVersion" => std::fs::read_to_string(sdk_path.join("version")).map_err(|error| Error::file(&sdk_path, &error))?,
            "sdkLibraries" => digests.tree(&sdk_path.join("lib"), &[])?,
        }));
        digests.save()?;
        let cached = store
            .read(&format!("cache/discovery/{discovery_key}.json"))
            .ok()
            .and_then(|record| record.get("discovery").cloned())
            .and_then(|value| Discovery::from_json(&value).ok());
        let discovery = match cached {
            Some(discovery) => {
                progress("Reusing checked plugin contributions");
                discovery
            }
            None => {
                progress("Discovering typed plugin contributions");
                let discovery = runner.discover(&graph, progress)?;
                store.write(
                    &format!("cache/discovery/{discovery_key}.json"),
                    object! {"discovery" => discovery.to_json()},
                )?;
                discovery
            }
        };
        for root in roots.keys() {
            if !discovery.plugins.contains(root) {
                return fail(format!(
                    "{root} contains no resolved @Plugin contribution library"
                ));
            }
        }
        let chosen: BTreeMap<String, String> = selections
            .iter()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap_or("").to_owned()))
            .collect();
        let ordered: BTreeMap<String, Vec<String>> = ordering
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    util::strings(value.as_array().map(Vec::as_slice).unwrap_or(&[])),
                )
            })
            .collect();
        let generated =
            Plan::resolve(&discovery, &chosen, &ordered, &[shell_application()])?.generate();
        let lib = app.join("lib");
        std::fs::create_dir_all(&lib)?;
        std::fs::write(lib.join("composition.g.dart"), &generated.source)?;
        let main = format!(
            "import 'package:denial_flutter_sdk/shell.dart';\nimport 'composition.g.dart' as composition;\nFuture<void> main() async {{\n  await runDenialShell(shell: composition.{}.single.createShell());\n}}\n",
            generated
                .contracts
                .get(&shell_application())
                .cloned()
                .unwrap_or_default()
        );
        std::fs::write(lib.join("main.dart"), &main)?;
        let marker = request.runtime.join(".denial-ui-source.json");
        let generation = if marker.exists() {
            read_json_map(&marker)?
                .get("flutter_generation")
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            let compositor =
                std::fs::read_to_string(request.runtime.join("compositor/core/src/lib.rs"))
                    .unwrap_or_default();
            compositor
                .split("FLUTTER_ENGINE_ABI: &str = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map_or(Value::Null, Value::from)
        };
        let lock = request
            .runtime
            .join("prebuilt/flutter-engine/SOURCE_LOCK.json");
        let mut version_arguments = request.flutter_arguments.to_vec();
        version_arguments.push("--version".into());
        version_arguments.push("--machine".into());
        let flutter_version: Value =
            serde_json::from_str(&util::run(&flutter, &version_arguments, None, None)?)
                .map_err(|error| Error::new(format!("Flutter did not say its version: {error}")))?;
        let unchanged = base
            .as_ref()
            .is_none_or(|base| same_composition(&selection, base));
        let mut metadata = object! {
            "flutterGeneration" => generation,
            "id" => request.candidate,
            // A rebuild of what is saved leaves nothing pending; a different
            // saved selection stays pending for Apply.
            "selectionRevision" => if unchanged { selection.get("revision").cloned().unwrap_or(Value::Null) } else { Value::Null },
        };
        if let Some(base) = request.rebuild_of {
            metadata.insert("rebuildOf".into(), base.into());
        }
        metadata.extend(object! {
            "roots" => resolved_roots,
            "selections" => selections.clone(),
            "ordering" => ordering.clone(),
            "discovery" => discovery.to_json(),
            "developmentSdk" => request.sdk_root.is_some(),
        });
        if let Some(sdk_root) = request.sdk_root {
            metadata.insert("sdkOverride".into(), display(sdk_root).into());
        }
        metadata.extend(object! {
            "runtimeRoot" => display(&util::absolute(request.runtime)),
            // What the installed Denial must be for this composition to
            // load: the packaged release, or this checkout.
            "sourceIdentity" => source_identity(request.runtime)?,
        });
        if lock.exists() {
            metadata.insert(
                "engineSourceLock".into(),
                Value::Object(read_json_map(&lock)?),
            );
        }
        metadata.extend(object! {
            "flutter" => flutter.clone(),
            "flutterVersion" => flutter_version.clone(),
            "status" => "planned",
        });
        let composition_key = content_key(&Value::Object(object! {
            "format" => 1,
            "discoveryKey" => discovery_key,
            "manifest" => manifest,
            "lock" => read_yaml_map(&app.join("pubspec.lock"))?,
            "generated" => generated.source.clone(),
            "main" => main,
            "sourceIdentity" => metadata["sourceIdentity"].clone(),
            "engineSourceLock" => metadata.get("engineSourceLock").cloned().unwrap_or(Value::Null),
            "framework" => flutter_version["frameworkRevision"].clone(),
            "selections" => selections,
            "ordering" => ordering,
        }));
        metadata.insert("compositionKey".into(), composition_key.into());
        store.write(
            &format!("candidates/{}/plan.json", request.candidate),
            metadata.clone(),
        )?;
        store.write("last-plan.json", object! {"id" => request.candidate})?;
        Ok(metadata)
    }
}

/// Every package's SDK constraints hold for the SDK snapshot; overrides
/// never bypass them.
fn validate_sdk_constraints(graph: &Graph, sdks: &[(&str, &Path)]) -> Result<()> {
    for package in &graph.packages {
        let dependencies = obj(package.pubspec.get("dependencies").unwrap_or(&Value::Null));
        for (sdk, folder) in sdks {
            let requested = match dependencies.get(*sdk) {
                Some(Value::String(constraint)) => Some(constraint.clone()),
                Some(Value::Object(spec)) => text(spec, "version").map(str::to_owned),
                _ => None,
            };
            let Some(requested) = requested else { continue };
            let version_text = text(&read_yaml_map(&folder.join("pubspec.yaml"))?, "version")
                .unwrap_or("")
                .to_owned();
            let version = version::parse_version(&version_text)?;
            if !version::allows(&requested, &version)? {
                return fail(format!(
                    "{} requires {sdk} {requested}; selected SDK is {version}. Overrides cannot bypass API constraints.",
                    package.name
                ));
            }
        }
    }
    Ok(())
}
