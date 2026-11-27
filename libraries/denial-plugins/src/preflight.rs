//! The quick check of a selection from what plugins declare in their
//! pubspecs (`denial_plugin` schema 1: `provides` and `requires`). No Pub,
//! analyzer, network or plugin code. A conflict is refused even when some
//! sources are unknown; a missing provider only when every source is known.
//! Discovery stays the authority while planning.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::object;
use crate::util::{self, Error, Kind, Map, Result, fail, normalize, obj, read_yaml_map, text};

/// What the application itself requires.
pub const SHELL_APPLICATION: &str = "package:denial_flutter_sdk/application.dart#ShellApplication";

fn is_contract(value: &str) -> bool {
    // package:NAME/PATH.dart#Type
    let Some(rest) = value.strip_prefix("package:") else {
        return false;
    };
    let Some((library, type_name)) = rest.split_once('#') else {
        return false;
    };
    let Some((package, path)) = library.split_once('/') else {
        return false;
    };
    crate::source::is_package_name(package)
        && path.ends_with(".dart")
        && path.len() > ".dart".len()
        && !path.contains(char::is_whitespace)
        && !path.contains('#')
        && type_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && type_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn contract(value: Option<&Value>) -> Result<String> {
    match value.and_then(Value::as_str) {
        Some(value) if is_contract(value) => Ok(value.to_owned()),
        _ => Err(Error::format(
            "Contracts must use package:library.dart#Type identities.",
        )),
    }
}

fn count(value: Option<&Value>, default: i64) -> Option<i64> {
    match value {
        None => Some(default),
        Some(value) => value.as_i64().filter(|_| value.is_i64() || value.is_u64()),
    }
}

struct Facts {
    providers: Map,
    requirements: Vec<Value>,
    issues: Vec<Value>,
    unknown: Vec<String>,
    dependencies: Vec<String>,
}

fn read_declarations(
    name: &str,
    directory: &Path,
    facts: &mut Facts,
    visit: &mut dyn FnMut(&str, &Path),
) -> Result<()> {
    let manifest = read_yaml_map(&directory.join("pubspec.yaml"))?;
    if text(&manifest, "name") != Some(name) {
        return Err(Error::format("Package name does not match."));
    }
    match manifest.get("denial_plugin") {
        None | Some(Value::Null) => {
            // Older plugins may not publish declarations yet.
            if !facts.unknown.iter().any(|known| known == name) {
                facts.unknown.push(name.to_owned());
            }
        }
        Some(declaration) => {
            let declaration_map = obj(declaration);
            let title = text(declaration_map, "name").unwrap_or("");
            if !declaration.is_object()
                || declaration_map.get("schema").and_then(Value::as_i64) != Some(1)
                || title.trim().is_empty()
            {
                return Err(Error::format(
                    "Expected denial_plugin schema 1 and a display name.",
                ));
            }
            let provided = declaration_map
                .get("provides")
                .cloned()
                .unwrap_or(Value::Array(vec![]));
            let required = declaration_map
                .get("requires")
                .cloned()
                .unwrap_or(Value::Array(vec![]));
            let (Value::Array(provided), Value::Array(required)) = (provided, required) else {
                return Err(Error::format("provides and requires must be lists."));
            };
            for value in &provided {
                if !value.is_object() {
                    return Err(Error::format("Invalid provider declaration."));
                }
                let id = contract(value.get("contract"))?;
                let count = count(value.get("count"), 1)
                    .filter(|count| (1..=1024).contains(count))
                    .ok_or_else(|| {
                        Error::format("Provider count must be a positive integer, at most 1024.")
                    })?;
                let names = facts
                    .providers
                    .entry(id)
                    .or_insert_with(|| Value::Array(vec![]));
                if let Value::Array(names) = names {
                    names.extend((0..count).map(|_| Value::from(title)));
                }
            }
            for value in &required {
                if !value.is_object() {
                    return Err(Error::format("Invalid requirement declaration."));
                }
                let id = contract(value.get("contract"))?;
                let min = count(value.get("min"), 1);
                let max = match value.get("max") {
                    None => Some(Value::from(1)),
                    Some(Value::Null) => Some(Value::Null),
                    Some(max) => max.as_i64().map(Value::from),
                };
                let valid = match (min, &max) {
                    (Some(min), Some(max)) => {
                        min >= 0
                            && max.as_i64().is_none_or(|max| max >= min)
                            && value.get("label").is_some_and(Value::is_string)
                    }
                    _ => false,
                };
                if !valid {
                    return Err(Error::format(
                        "Requirements need a label and valid min/max counts (max may be null).",
                    ));
                }
                facts.requirements.push(Value::Object(object! {
                    "contract" => id,
                    "label" => value["label"].clone(),
                    "owner" => title,
                    "min" => min.unwrap_or(1),
                    "max" => max.unwrap_or(Value::Null),
                }));
            }
        }
    }
    if let Some(dependencies) = manifest.get("dependencies").and_then(Value::as_object) {
        for (dependency, spec) in dependencies {
            // Platform packages are never user-selected implementations.
            if dependency == "denial_sdk"
                || dependency == "denial_flutter_sdk"
                || spec.get("sdk").is_some_and(|sdk| !sdk.is_null())
            {
                continue;
            }
            if let Some(path) = spec.get("path").and_then(Value::as_str) {
                facts.dependencies.push(dependency.clone());
                visit(dependency, &normalize(&directory.join(path)));
            } else if !facts.unknown.contains(dependency) {
                // Pub alone decides hosted and Git versions.
                facts.unknown.push(dependency.clone());
            }
        }
    }
    Ok(())
}

/// The declarations of every plugin in `roots` (name to record with a
/// `directory`) and of their path dependencies.
pub fn describe(roots: &Map) -> Value {
    let mut packages = Map::new();
    fn visit(name: &str, directory: &Path, packages: &mut Map) {
        if packages.contains_key(name) {
            return;
        }
        packages.insert(name.to_owned(), Value::Null);
        let mut facts = Facts {
            providers: Map::new(),
            requirements: Vec::new(),
            issues: Vec::new(),
            unknown: Vec::new(),
            dependencies: Vec::new(),
        };
        let mut nested = Vec::new();
        let result = read_declarations(name, directory, &mut facts, &mut |dependency, path| {
            nested.push((dependency.to_owned(), path.to_path_buf()));
        });
        // Dependencies are visited after this package's record exists.
        match result {
            Ok(()) => {}
            Err(error) if error.kind == Kind::FileSystem => {
                if !facts.unknown.iter().any(|known| known == name) {
                    facts.unknown.push(name.to_owned());
                }
            }
            Err(error) => {
                let shown = if error.kind == Kind::Format {
                    format!("FormatException: {}", error.message)
                } else {
                    error.message
                };
                facts.issues.push(Value::Object(object! {
                    "code" => "invalid_declaration",
                    "title" => "Plugin information needs fixing",
                    "message" => format!("{name} has invalid compatibility declarations. {shown}"),
                    "plugins" => vec![name.to_owned()],
                }));
            }
        }
        packages.insert(
            name.to_owned(),
            Value::Object(object! {
                "providers" => facts.providers,
                "requirements" => facts.requirements,
                "issues" => facts.issues,
                "unknown" => facts.unknown,
                "dependencies" => facts.dependencies,
            }),
        );
        for (dependency, path) in nested {
            visit(&dependency, &path, packages);
        }
    }
    for (name, value) in roots {
        if let Some(directory) = value.get("directory").and_then(Value::as_str) {
            visit(name, Path::new(directory), &mut packages);
        }
    }
    Value::Object(packages)
}

/// Checks `selection` against the declarations `packages` describes.
pub fn check_selection(selection: &Map, packages: &Value) -> Map {
    let packages = obj(packages);
    let mut providers: Map = Map::new();
    let mut requirements = vec![Value::Object(object! {
        "contract" => SHELL_APPLICATION,
        "label" => "Desktop",
        "owner" => "Denial",
        "min" => 1,
        "max" => 1,
    })];
    let mut issues: Vec<Value> = Vec::new();
    let mut unknown = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let choices = obj(selection.get("selections").unwrap_or(&Value::Null));
    fn visit(
        name: &str,
        packages: &Map,
        visited: &mut BTreeSet<String>,
        unknown: &mut BTreeSet<String>,
        providers: &mut Map,
        requirements: &mut Vec<Value>,
        issues: &mut Vec<Value>,
    ) {
        if !visited.insert(name.to_owned()) {
            return;
        }
        let Some(facts) = packages.get(name).and_then(Value::as_object) else {
            unknown.insert(name.to_owned());
            return;
        };
        for (contract, names) in obj(facts.get("providers").unwrap_or(&Value::Null)) {
            if let Value::Array(list) = providers
                .entry(contract.clone())
                .or_insert_with(|| Value::Array(vec![]))
            {
                list.extend(names.as_array().cloned().unwrap_or_default());
            }
        }
        requirements.extend(util::list(facts, "requirements").iter().cloned());
        issues.extend(util::list(facts, "issues").iter().cloned());
        unknown.extend(util::strings(util::list(facts, "unknown")));
        for dependency in util::strings(util::list(facts, "dependencies")) {
            visit(
                &dependency,
                packages,
                visited,
                unknown,
                providers,
                requirements,
                issues,
            );
        }
    }
    for name in obj(selection.get("roots").unwrap_or(&Value::Null)).keys() {
        visit(
            name,
            packages,
            &mut visited,
            &mut unknown,
            &mut providers,
            &mut requirements,
            &mut issues,
        );
    }
    for requirement in &requirements {
        let id = requirement["contract"].as_str().unwrap_or("");
        // An explicit provider choice is checked with real types later.
        if choices.contains_key(id) {
            continue;
        }
        let names = util::strings(
            providers
                .get(id)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        let max = requirement["max"].as_i64();
        let min = requirement["min"].as_i64().unwrap_or(0);
        let label = requirement["label"].as_str().unwrap_or("");
        let owner = requirement["owner"].as_str().unwrap_or("");
        let mut distinct: Vec<String> = Vec::new();
        for name in &names {
            if !distinct.contains(name) {
                distinct.push(name.clone());
            }
        }
        if let Some(max) = max.filter(|max| names.len() as i64 > *max) {
            let accepts = if max == 1 {
                "only one".to_owned()
            } else {
                format!("at most {max}")
            };
            let disable = if names.len() as i64 - max == 1 {
                "one of these plugins"
            } else {
                "extra plugins"
            };
            issues.push(Value::Object(object! {
                "code" => "too_many_providers",
                "contract" => id,
                "title" => format!("{label} conflict"),
                "message" => format!(
                    "{label} is provided by {}. {owner} accepts {accepts}. Disable {disable} before applying.",
                    distinct.join(" and ")
                ),
                "plugins" => distinct,
                "requirement" => requirement.clone(),
            }));
        } else if (names.len() as i64) < min && unknown.is_empty() {
            let needs = if min == 1 {
                "a provider for".to_owned()
            } else {
                format!("at least {min} providers for")
            };
            issues.push(Value::Object(object! {
                "code" => "missing_provider",
                "contract" => id,
                "title" => format!("{label} is missing"),
                "message" => format!(
                    "{owner} needs {needs} {label}. Enable a compatible plugin before applying."
                ),
                "plugins" => Vec::<String>::new(),
                "requirement" => requirement.clone(),
            }));
        }
    }
    object! {
        "selectionRevision" => selection.get("revision").cloned().unwrap_or(Value::Null),
        "canApply" => issues.is_empty(),
        "complete" => unknown.is_empty(),
        "issues" => issues,
        "uncheckedPackages" => unknown.into_iter().collect::<Vec<_>>(),
    }
}

/// The check of `selection`'s own roots.
pub fn check(selection: &Map) -> Map {
    check_selection(
        selection,
        &describe(obj(selection.get("roots").unwrap_or(&Value::Null))),
    )
}

/// Fails with the first problem of `selection`.
pub fn require_valid(selection: &Map) -> Result<()> {
    let result = check(selection);
    match util::list(&result, "issues").first() {
        Some(issue) => fail(issue["message"].as_str().unwrap_or("").to_owned()),
        None => Ok(()),
    }
}
