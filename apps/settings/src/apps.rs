//! The apps on the phone, as their desktop entries list them, with their
//! icons found as Denial's launcher finds them: a picture in `pixmaps`, then
//! the icon themes, hicolor and Adwaita first, the largest size first.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use denial_look::words::{Language, language};

#[derive(Clone, Debug)]
pub struct App {
    /// The desktop entry's id: "dev.denial.Settings.desktop".
    pub id: String,
    pub name: String,
    pub icon: Option<PathBuf>,
    /// For an Android app Droidloom put in the launcher: its package and
    /// launcher activity.
    pub android: Option<Android>,
}

#[derive(Clone, Debug)]
pub struct Android {
    pub package: String,
    pub component: Option<String>,
}

/// The data folders, the user's first.
fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let user = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    user.into_iter()
        .chain(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .collect()
}

/// The apps the launcher would show, by name.
pub fn read() -> Vec<App> {
    let dirs = data_dirs();
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in &dirs {
        let mut entries = Vec::new();
        collect(&dir.join("applications"), "", &mut entries);
        for (id, path) in entries {
            // An entry in an earlier folder hides one with its id after.
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(app) = entry(&id, &path, &dirs) {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// The desktop entries under `folder`, with their ids: a subfolder's name
/// and a dash before the file's.
fn collect(folder: &Path, prefix: &str, entries: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect(&path, &format!("{prefix}{name}-"), entries);
        } else if name.ends_with(".desktop") {
            entries.push((format!("{prefix}{name}"), path));
        }
    }
}

fn entry(id: &str, path: &Path, dirs: &[PathBuf]) -> Option<App> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut fields = std::collections::HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert(value.trim());
        }
    }
    let yes = |key| fields.get(key).is_some_and(|value| *value == "true");
    let lists_denial = |key| {
        fields
            .get(key)
            .map(|list| list.split(';').any(|desktop| desktop == "Denial"))
    };
    if fields.get("Type") != Some(&"Application")
        || yes("NoDisplay")
        || yes("Hidden")
        || lists_denial("OnlyShowIn") == Some(false)
        || lists_denial("NotShowIn") == Some(true)
    {
        return None;
    }
    let name = match language() {
        Language::Chinese => ["Name[zh_CN]", "Name[zh]"]
            .into_iter()
            .find_map(|key| fields.get(key))
            .or_else(|| fields.get("Name")),
        Language::English => fields.get("Name"),
    }?;
    Some(App {
        id: id.to_owned(),
        name: (*name).to_owned(),
        icon: fields
            .get("Icon")
            .filter(|icon| !icon.is_empty())
            .and_then(|icon| icon_path(icon, dirs)),
        android: fields
            .get("X-Droidloom-Package")
            .filter(|_| yes("X-Droidloom-Managed"))
            .map(|package| Android {
                package: (*package).to_owned(),
                component: fields
                    .get("X-Droidloom-Component")
                    .map(|component| (*component).to_owned()),
            }),
    })
}

const PICTURES: [&str; 3] = ["png", "svg", "jpg"];

/// The file of the icon named `icon`, or `icon` itself when it is a path.
fn icon_path(icon: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if icon.starts_with('/') {
        return Path::new(icon).is_file().then(|| PathBuf::from(icon));
    }
    for dir in dirs {
        for extension in PICTURES {
            let picture = dir.join(format!("pixmaps/{icon}.{extension}"));
            if picture.is_file() {
                return Some(picture);
            }
        }
        let icons = dir.join("icons");
        let mut themes = vec![icons.join("hicolor"), icons.join("Adwaita")];
        if let Ok(read) = std::fs::read_dir(&icons) {
            themes.extend(read.flatten().map(|entry| entry.path()));
        }
        for theme in themes {
            if let Some(found) = in_theme(&theme, icon) {
                return Some(found);
            }
        }
    }
    None
}

/// `icon` in a theme, the sharpest at the size Settings draws it: a picture
/// of 128 pixels or more, then the largest drawing, then the largest
/// smaller picture. GPUI draws a drawing at its own size. Themes keep their folders by size
/// then kind (hicolor's `48x48/apps`) or by kind then size (Breeze's
/// `apps/48`).
fn in_theme(theme: &Path, icon: &str) -> Option<PathBuf> {
    let size = |name: &str| match name {
        "scalable" => Some(512),
        _ => name.split('x').next()?.parse::<u32>().ok(),
    };
    let folders = |path: &Path| {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                )
            })
            .collect::<Vec<_>>()
    };
    let mut found: Vec<((u8, u32), PathBuf)> = Vec::new();
    for (outer, path) in folders(theme) {
        for (inner, folder) in folders(&path) {
            let Some(pixels) = size(&outer).or_else(|| size(&inner)) else {
                continue;
            };
            for extension in PICTURES {
                let file = folder.join(format!("{icon}.{extension}"));
                if file.is_file() {
                    let rank = match (extension, pixels) {
                        (_, 128..) if extension != "svg" => (2, pixels),
                        ("svg", _) => (1, pixels),
                        _ => (0, pixels),
                    };
                    found.push((rank, file));
                }
            }
        }
    }
    found
        .into_iter()
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, file)| file)
}

#[cfg(test)]
mod tests {
    /// Lists the apps on a phone: `cargo test -- --ignored --nocapture
    /// lists_apps`, as the session's user.
    #[test]
    #[ignore]
    fn lists_apps() {
        for app in super::read() {
            println!("{} {} {:?}", app.id, app.name, app.icon);
        }
    }
}
