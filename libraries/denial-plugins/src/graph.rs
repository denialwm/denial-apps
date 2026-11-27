//! The runtime package graph Pub resolved for a plugin workspace. Pub owns
//! resolution; this only walks what it wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::util::{self, Map, Result, fail, normalize, read_json_map, read_yaml_map, text};

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub root: PathBuf,
    pub library_root: PathBuf,
    pub pubspec: Map,
}

impl Package {
    /// The names this package depends on at run time, sorted.
    pub fn dependencies(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .pubspec
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub application: String,
    /// In the order they were reached from the application.
    pub packages: Vec<Package>,
    /// For each package, the names from the application down to it.
    pub chains: BTreeMap<String, Vec<String>>,
}

/// A `file:` URI or a relative reference, resolved against `base`, a folder.
fn resolve_uri(base: &Path, reference: &str) -> PathBuf {
    let decoded = percent_decode(reference);
    if let Some(path) = decoded.strip_prefix("file://") {
        normalize(Path::new(path))
    } else {
        normalize(&base.join(decoded))
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(byte) = value
                .get(index + 1..index + 3)
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        {
            result.push(byte);
            index += 3;
            continue;
        }
        result.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

impl Graph {
    pub fn read(workspace: &Path) -> Result<Self> {
        let workspace = util::absolute(workspace);
        let config_path = workspace.join(".dart_tool/package_config.json");
        let config = read_json_map(&config_path)?;
        if config.get("configVersion") != Some(&Value::from(2)) {
            return fail("Unsupported Pub package configuration");
        }
        let config_folder = config_path.parent().unwrap_or(&workspace).to_path_buf();
        let mut all = BTreeMap::new();
        for record in util::list(&config, "packages") {
            let record = util::obj(record);
            let name = text(record, "name").unwrap_or("").to_owned();
            let root = resolve_uri(&config_folder, text(record, "rootUri").unwrap_or(""));
            let lib = resolve_uri(&root, text(record, "packageUri").unwrap_or("lib/"));
            let manifest = read_yaml_map(&root.join("pubspec.yaml"))?;
            if text(&manifest, "name") != Some(name.as_str()) {
                return fail(format!(
                    "Pub package {name} declares a different name in {}",
                    root.display()
                ));
            }
            all.insert(
                name.clone(),
                Package {
                    name,
                    root,
                    library_root: lib,
                    pubspec: manifest,
                },
            );
        }
        let application = text(&read_yaml_map(&workspace.join("pubspec.yaml"))?, "name")
            .unwrap_or("")
            .to_owned();
        let mut chains = BTreeMap::new();
        chains.insert(application.clone(), vec![application.clone()]);
        let mut pending = vec![application.clone()];
        let mut packages = Vec::new();
        let mut index = 0;
        while index < pending.len() {
            let name = pending[index].clone();
            index += 1;
            let Some(package) = all.get(&name) else {
                return fail(format!(
                    "Unresolved runtime dependency: {}. Run Pub first.",
                    chains[&name].join(" -> ")
                ));
            };
            for dependency in package.dependencies() {
                if !chains.contains_key(&dependency) {
                    let mut chain = chains[&name].clone();
                    chain.push(dependency.clone());
                    chains.insert(dependency.clone(), chain);
                    pending.push(dependency);
                }
            }
            packages.push(package.clone());
        }
        Ok(Self {
            application,
            packages,
            chains,
        })
    }

    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.name == name)
    }

    /// The `package:` URI of the library at `path`.
    pub fn library_uri(&self, path: &Path) -> Result<String> {
        let absolute = util::absolute(path);
        for package in &self.packages {
            let lib = normalize(&package.library_root);
            if util::is_within(&lib, &absolute) {
                let relative = absolute.strip_prefix(&lib).unwrap_or(&absolute);
                return Ok(format!(
                    "package:{}/{}",
                    package.name,
                    relative.to_string_lossy()
                ));
            }
        }
        fail(format!(
            "Declaration outside runtime package libraries: {}",
            path.display()
        ))
    }
}
