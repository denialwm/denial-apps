//! The plugins one can use: the ones Denial comes with, and the community
//! list. The list says only where plugins are; Pub decides versions and
//! dependencies.

use std::path::Path;

use serde_json::Value;

use crate::object;
use crate::preflight;
use crate::source::{Kind, Repositories, Resolved, Source};
use crate::store::Store;
use crate::util::{self, Map, Result, encode, fail, obj, read_yaml_map, text};

/// The community list Denial ships with.
pub const DEFAULT_CATALOG: &str = "https://github.com/denialwm/denial-plugins.git";

/// The catalog settings: the shipped list unless the user set another one,
/// or none (a null repository).
pub fn catalog_configuration(settings: &Map) -> Map {
    let mut configuration = object! {
        "catalog-git" => DEFAULT_CATALOG,
        "catalog-file" => "plugins.yaml",
    };
    for (key, value) in settings {
        configuration.insert(key.clone(), value.clone());
    }
    configuration
}

/// The sources a catalog file lists.
pub fn read_catalog(path: &Path) -> Result<Vec<Source>> {
    let value = read_yaml_map(path)?;
    let (Some(Value::Array(plugins)), Some(1)) = (
        value.get("plugins"),
        value.get("schema").and_then(Value::as_i64),
    ) else {
        return fail("Unsupported plugin catalog schema");
    };
    let mut sources = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in plugins {
        let entry = obj(item);
        if !item.is_object()
            || !entry.get("git").is_some_and(Value::is_string)
            || entry
                .keys()
                .any(|key| !["git", "path", "ref"].contains(&key.as_str()))
        {
            return fail("Catalog entries accept only git, path, and ref");
        }
        let source = Source::new(
            Kind::Git,
            text(entry, "git").unwrap_or(""),
            text(entry, "path").unwrap_or("."),
            text(entry, "ref"),
        )?;
        if !seen.insert(source.identity()) {
            return fail("Duplicate plugin catalog entry");
        }
        sources.push(source);
    }
    Ok(sources)
}

pub struct Available<'a> {
    pub store: &'a Store,
    pub repositories: &'a Repositories,
}

impl Available<'_> {
    /// The plugins `runtime` comes with, resolved, with `builtin` and
    /// `default`.
    pub fn builtins(&self, runtime: Option<&str>) -> Result<Vec<Map>> {
        let Some(runtime) = runtime else {
            return Ok(Vec::new());
        };
        let manifest = Path::new(runtime).join("plugins/builtins.yaml");
        if !manifest.exists() {
            return fail("Installed source has no built-in plugin manifest");
        }
        let value = read_yaml_map(&manifest)?;
        if value.get("schema").and_then(Value::as_i64) != Some(1) {
            return fail("Unsupported built-in plugin manifest");
        }
        let mut result = Vec::new();
        for item in util::list(&value, "plugins") {
            let entry = obj(item);
            let source = Source::new(
                Kind::Builtin,
                &util::display(&Path::new(runtime).join("plugins")),
                text(entry, "path").unwrap_or(""),
                None,
            )?;
            let mut resolved = self.repositories.resolve(&source, None)?.to_json();
            resolved.insert("builtin".into(), true.into());
            resolved.insert(
                "default".into(),
                (entry.get("default") == Some(&Value::Bool(true))).into(),
            );
            result.push(resolved);
        }
        Ok(result)
    }

    /// The built-ins and the community list, which is read once, on first
    /// use, and then only when asked.
    pub fn list(&self, configuration: &Map) -> Result<Map> {
        let configuration = catalog_configuration(configuration);
        let builtins = self.builtins(text(&configuration, "runtime"))?;
        let configured = text(&configuration, "catalog-git").is_some_and(|url| !url.is_empty());
        let mut catalog = self.store.read("catalog.json")?;
        if configured && catalog.is_empty() {
            catalog = self.store.exclusive(|| {
                let cached = self.store.read("catalog.json")?;
                if !cached.is_empty() {
                    return Ok(cached);
                }
                match self.refresh(&configuration) {
                    Ok(fresh) => Ok(fresh),
                    Err(error) => {
                        // A first failure leaves the built-ins usable and is
                        // not retried at every poll; Refresh tries again.
                        self.store.write(
                            "catalog.json",
                            object! {"entries" => Vec::<Value>::new(), "errors" => vec![error.message]},
                        )?;
                        self.store.read("catalog.json")
                    }
                }
            })?;
        }
        let mut known = Map::new();
        for plugin in builtins
            .iter()
            .cloned()
            .map(Value::Object)
            .chain(util::list(&catalog, "entries").iter().cloned())
        {
            if let Some(name) = plugin["name"].as_str() {
                known.insert(name.to_owned(), plugin.clone());
            }
        }
        Ok(object! {
            "builtins" => builtins.into_iter().map(Value::Object).collect::<Vec<_>>(),
            "catalog" => catalog,
            "configured" => configured,
            "declarations" => preflight::describe(&known),
        })
    }

    /// Reads the community list again.
    pub fn refresh(&self, configuration: &Map) -> Result<Map> {
        let configuration = catalog_configuration(configuration);
        let url = match text(&configuration, "catalog-git") {
            Some(url) if !url.is_empty() => url,
            _ => return fail("No community catalog repository is configured"),
        };
        let source = Source::new(Kind::Git, url, ".", text(&configuration, "catalog-ref"))?;
        let file = text(&configuration, "catalog-file").unwrap_or("plugins.yaml");
        if file.starts_with('/') || file.split('/').any(|part| part == "..") {
            return fail("Catalog path must stay in the repository");
        }
        let (directory, _) = self.repositories.checkout(&source, None)?;
        let mut entries = Vec::new();
        let mut errors = Vec::new();
        for plugin in read_catalog(&directory.join(file))? {
            match self.repositories.resolve(&plugin, None) {
                Ok(resolved) => entries.push(Value::Object(resolved.to_json())),
                Err(error) => errors.push(format!("{}: {error}", plugin.location)),
            }
        }
        let result = object! {
            "source" => source.to_json(),
            "entries" => entries,
            "errors" => errors,
            "updated" => util::now(),
        };
        self.store.write("catalog.json", result.clone())?;
        Ok(result)
    }

    /// Points the records of built-in plugins at `runtime`'s sources. An
    /// existing user never gets plugins a newer Denial added selected.
    pub fn adopt_builtins(&self, runtime: &str) -> Result<()> {
        let selection = self.store.selection()?;
        let mut roots = obj(&selection["roots"]).clone();
        let installed = self
            .store
            .read_or("installed.json", object! {"plugins" => roots.clone()})?;
        let mut known = obj(&installed["plugins"]).clone();
        let before = encode(&Value::Object(known.clone()));
        let builtin =
            |value: Option<&Value>| value.is_some_and(|value| value["source"]["kind"] == "builtin");
        let mut changed = false;
        for mut entry in self.builtins(Some(runtime))? {
            let name = text(&entry, "name").unwrap_or("").to_owned();
            entry.shift_remove("builtin");
            entry.shift_remove("default");
            let value = Value::Object(entry);
            if !known.contains_key(&name) || builtin(known.get(&name)) {
                known.insert(name.clone(), value.clone());
            }
            if builtin(roots.get(&name)) && roots.get(&name).map(encode) != Some(encode(&value)) {
                roots.insert(name, value);
                changed = true;
            }
        }
        if encode(&Value::Object(known.clone())) != before
            || !self.store.file("installed.json").exists()
        {
            self.store
                .write("installed.json", object! {"plugins" => known})?;
        }
        if changed {
            self.store.write_selection(&selection, roots)?;
        }
        Ok(())
    }

    /// Makes the selection the plugins Denial comes with, and only them.
    /// What was turned off stays known.
    pub fn defaults(&self, runtime: Option<&str>) -> Result<()> {
        let mut defaults: Vec<Resolved> = Vec::new();
        for item in self.builtins(runtime)? {
            if item.get("default") == Some(&Value::Bool(true)) {
                let source = Source::from_json(&item["source"])?;
                defaults.push(self.repositories.resolve(&source, None)?);
            }
        }
        let names: Vec<&str> = defaults.iter().map(|plugin| plugin.name.as_str()).collect();
        let roots = obj(&self.store.selection()?["roots"]).clone();
        for (name, root) in &roots {
            if !names.contains(&name.as_str()) || root["source"]["kind"] != "builtin" {
                self.store.remove(name)?;
            }
        }
        for plugin in &defaults {
            if !obj(&self.store.selection()?["roots"]).contains_key(&plugin.name) {
                self.store.select(plugin)?;
            }
        }
        Ok(())
    }
}
