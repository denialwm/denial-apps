//! Plugin discovery: which libraries are plugins, what they provide and what
//! their constructors take. Only Dart's analyzer can resolve annotations,
//! aliases and constants, so this runs Denial's discovery script,
//! `packages/denial_plugin_discovery/discover.dart` of the runtime, with the
//! Flutter tool's analyzer. The script is not compiled with Denial: the
//! first run here keeps a kernel snapshot of it in the manager's cache.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::graph::Graph;
use crate::object;
use crate::util::{self, Map, Result, display, fail, obj, sha256, text};

/// Where the script is, inside a runtime: a kit's or a checkout.
pub const SCRIPT: &str = "packages/denial_plugin_discovery/discover.dart";

/// A type named by its declaring library, never by its bare name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TypeId {
    pub library: String,
    pub name: String,
}

impl TypeId {
    pub fn key(&self) -> String {
        format!("{}#{}", self.library, self.name)
    }

    fn from_json(value: &Value) -> Result<Self> {
        let map = obj(value);
        match (text(map, "library"), text(map, "name")) {
            (Some(library), Some(name)) => Ok(Self {
                library: library.to_owned(),
                name: name.to_owned(),
            }),
            _ => fail("Invalid plugin discovery: a type without its library"),
        }
    }

    fn to_json(&self) -> Value {
        Value::Object(object! {"library" => self.library.clone(), "name" => self.name.clone()})
    }
}

/// Ordered by key, as Dart's manager ordered them.
impl Ord for TypeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for TypeId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.key())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    ExactlyOne,
    ZeroOrOne,
    ZeroOrMore,
}

impl Cardinality {
    fn name(self) -> &'static str {
        match self {
            Cardinality::ExactlyOne => "exactlyOne",
            Cardinality::ZeroOrOne => "zeroOrOne",
            Cardinality::ZeroOrMore => "zeroOrMore",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub type_id: TypeId,
    pub cardinality: Cardinality,
}

#[derive(Clone, Debug)]
pub struct Injection {
    pub name: String,
    pub contract: TypeId,
    pub named: bool,
    pub many: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug)]
pub struct Contribution {
    pub package: String,
    pub type_id: TypeId,
    pub contracts: Vec<TypeId>,
    pub arguments: Vec<Injection>,
}

/// Every declaration discovery found, serialized always the same way.
#[derive(Clone, Debug)]
pub struct Discovery {
    pub plugins: Vec<String>,
    pub contracts: Vec<Contract>,
    pub contributions: Vec<Contribution>,
    pub chains: BTreeMap<String, Vec<String>>,
}

impl Discovery {
    pub fn from_json(value: &Value) -> Result<Self> {
        let map = obj(value);
        let mut contracts = Vec::new();
        for item in util::list(map, "contracts") {
            let cardinality = match item["cardinality"].as_str() {
                Some("exactlyOne") => Cardinality::ExactlyOne,
                Some("zeroOrOne") => Cardinality::ZeroOrOne,
                Some("zeroOrMore") => Cardinality::ZeroOrMore,
                _ => return fail("Invalid plugin discovery: unknown cardinality"),
            };
            contracts.push(Contract {
                type_id: TypeId::from_json(&item["type"])?,
                cardinality,
            });
        }
        let mut contributions = Vec::new();
        for item in util::list(map, "contributions") {
            let item = obj(item);
            let mut arguments = Vec::new();
            for argument in util::list(item, "arguments") {
                arguments.push(Injection {
                    name: argument["name"].as_str().unwrap_or("").to_owned(),
                    contract: TypeId::from_json(&argument["contract"])?,
                    named: argument["named"] == true,
                    many: argument["many"] == true,
                    nullable: argument["nullable"] == true,
                });
            }
            contributions.push(Contribution {
                package: text(item, "package").unwrap_or("").to_owned(),
                type_id: TypeId::from_json(item.get("type").unwrap_or(&Value::Null))?,
                contracts: util::list(item, "contracts")
                    .iter()
                    .map(TypeId::from_json)
                    .collect::<Result<_>>()?,
                arguments,
            });
        }
        let chains = obj(map.get("dependencyChains").unwrap_or(&Value::Null))
            .iter()
            .map(|(name, chain)| {
                (
                    name.clone(),
                    util::strings(chain.as_array().map(Vec::as_slice).unwrap_or(&[])),
                )
            })
            .collect();
        Ok(Self {
            plugins: util::strings(util::list(map, "plugins")),
            contracts,
            contributions,
            chains,
        })
    }

    pub fn to_json(&self) -> Value {
        Value::Object(object! {
            "plugins" => self.plugins.clone(),
            "contracts" => self.contracts.iter().map(|contract| Value::Object(object! {
                "type" => contract.type_id.to_json(),
                "cardinality" => contract.cardinality.name(),
            })).collect::<Vec<_>>(),
            "contributions" => self.contributions.iter().map(|contribution| Value::Object(object! {
                "package" => contribution.package.clone(),
                "type" => contribution.type_id.to_json(),
                "contracts" => contribution.contracts.iter().map(TypeId::to_json).collect::<Vec<_>>(),
                "arguments" => contribution.arguments.iter().map(|argument| Value::Object(object! {
                    "name" => argument.name.clone(),
                    "contract" => argument.contract.to_json(),
                    "named" => argument.named,
                    "many" => argument.many,
                    "nullable" => argument.nullable,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "dependencyChains" => self.chains.iter()
                .map(|(name, chain)| (name.clone(), Value::from(chain.clone())))
                .collect::<Map>(),
        })
    }
}

/// How discovery runs: the script, the Dart SDK the Flutter tool uses, and
/// that tool's package configuration, which resolves the analyzer.
pub struct Runner {
    pub script: PathBuf,
    pub flutter: PathBuf,
    /// Where analysis results and the script's snapshot are kept.
    pub cache: PathBuf,
}

impl Runner {
    /// For a runtime (a kit's or a checkout) and its Flutter launcher.
    pub fn new(runtime: &Path, flutter: &Path, cache: PathBuf) -> Result<Self> {
        let flutter =
            std::fs::canonicalize(flutter).map_err(|error| util::Error::file(flutter, &error))?;
        Ok(Self {
            script: runtime.join(SCRIPT),
            flutter,
            cache,
        })
    }

    fn flutter_root(&self) -> PathBuf {
        self.flutter
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("/"))
            .to_path_buf()
    }

    /// The Dart SDK the Flutter tool runs on: what discovery analyzes against.
    pub fn sdk(&self) -> PathBuf {
        self.flutter_root().join("bin/cache/dart-sdk")
    }

    fn packages(&self) -> PathBuf {
        self.flutter_root()
            .join("packages/flutter_tools/.dart_tool/package_config.json")
    }

    /// What identifies the script's results: its source and the Dart and
    /// analyzer it runs with.
    pub fn identity(&self) -> Result<String> {
        let script =
            std::fs::read(&self.script).map_err(|error| util::Error::file(&self.script, &error))?;
        let version = std::fs::read(self.sdk().join("version")).unwrap_or_default();
        let packages = std::fs::read(self.packages()).unwrap_or_default();
        Ok(sha256(
            &[sha256(&script), sha256(&version), sha256(&packages)]
                .join("\0")
                .into_bytes(),
        ))
    }

    /// The script's kernel snapshot, made on first use. None when it cannot
    /// be made: the script then runs from source.
    fn snapshot(&self, progress: &dyn Fn(&str)) -> Option<PathBuf> {
        let identity = self.identity().ok()?;
        let folder = self.cache.join("discovery-script");
        let snapshot = folder.join(format!("{identity}.dill"));
        if snapshot.is_file() {
            return Some(snapshot);
        }
        progress("Preparing plugin discovery");
        std::fs::create_dir_all(&folder).ok()?;
        let staged = folder.join(format!("{identity}.{}.new", std::process::id()));
        let made = Command::new(self.sdk().join("bin/dart"))
            .arg("--snapshot-kind=kernel")
            .arg(format!("--snapshot={}", staged.display()))
            .arg(format!("--packages={}", self.packages().display()))
            .arg(&self.script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .ok()?;
        if !made.status.success() || std::fs::rename(&staged, &snapshot).is_err() {
            let _ = std::fs::remove_file(&staged);
            return None;
        }
        // One snapshot at a time: an older one never runs again.
        if let Ok(listing) = std::fs::read_dir(&folder) {
            for entry in listing.flatten() {
                if entry.path() != snapshot {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Some(snapshot)
    }

    fn run(&self, program: &[PathBuf], request: &[u8]) -> Result<std::process::Output> {
        let mut child = Command::new(self.sdk().join("bin/dart"))
            .args(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                util::Error::new(format!("Plugin discovery did not start: {error}"))
            })?;
        let mut stdin = child.stdin.take().expect("piped");
        let request = request.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&request);
        });
        let output = child.wait_with_output()?;
        let _ = writer.join();
        Ok(output)
    }

    /// Discovers the plugins of the resolved workspace `graph`.
    pub fn discover(&self, graph: &Graph, progress: &dyn Fn(&str)) -> Result<Discovery> {
        if graph.package("denial_sdk").is_none() {
            return fail("The runtime graph must depend on denial_sdk");
        }
        // Only packages that reach denial_sdk can contribute plugins.
        let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for package in &graph.packages {
            for dependency in package.dependencies() {
                dependents
                    .entry(dependency)
                    .or_default()
                    .push(package.name.clone());
            }
        }
        let mut eligible = BTreeSet::from(["denial_sdk".to_owned()]);
        let mut pending = vec!["denial_sdk".to_owned()];
        while let Some(name) = pending.pop() {
            for dependent in dependents.get(&name).into_iter().flatten() {
                if eligible.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
        let application = graph
            .package(&graph.application)
            .map(|package| display(&package.root))
            .unwrap_or_default();
        let request = Value::Object(object! {
            "sdk" => display(&self.sdk()),
            "cache" => display(&self.cache.join("analyzer")),
            "application" => application,
            "packages" => graph.packages.iter().map(|package| Value::Object(object! {
                "name" => package.name.clone(),
                "lib" => display(&package.library_root),
            })).collect::<Vec<_>>(),
            "eligible" => eligible.into_iter().collect::<Vec<_>>(),
        });
        let request = util::encode(&request).into_bytes();
        let source = vec![
            PathBuf::from(format!("--packages={}", self.packages().display())),
            self.script.clone(),
        ];
        let output = match self.snapshot(progress) {
            Some(snapshot) => {
                let output = self.run(std::slice::from_ref(&snapshot), &request)?;
                // A snapshot this Dart no longer loads runs from source.
                let said = String::from_utf8_lossy(&output.stderr);
                if output.status.success()
                    || output.status.code() == Some(1)
                    || !(said.contains("ernel") || said.contains("snapshot"))
                {
                    output
                } else {
                    let _ = std::fs::remove_file(&snapshot);
                    self.run(&source, &request)?
                }
            }
            None => self.run(&source, &request)?,
        };
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            let said = said.trim();
            return fail(if said.is_empty() {
                "Plugin discovery failed".to_owned()
            } else {
                said.to_owned()
            });
        }
        let answer: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            util::Error::new(format!("Plugin discovery answered badly: {error}"))
        })?;
        let mut discovery = Discovery::from_json(&answer)?;
        discovery.chains = graph.chains.clone();
        Ok(discovery)
    }
}
