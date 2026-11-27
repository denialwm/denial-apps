//! The app an Access request is about: its name and icon from its desktop
//! entry, found as Settings finds the launcher's apps. Its icon is a
//! picture in `pixmaps`, else the icon themes, hicolor and Adwaita first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use denial_look::words::{Language, language};

pub struct App {
    pub name: String,
    pub icon: Option<PathBuf>,
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

/// The app whose desktop entry is `app_id.desktop`, or whose
/// `StartupWMClass` is `app_id`.
pub fn find(app_id: &str) -> Option<App> {
    if app_id.is_empty() || app_id.contains('/') {
        return None;
    }
    let dirs = data_dirs();
    for dir in &dirs {
        let path = dir.join(format!("applications/{app_id}.desktop"));
        if let Some(fields) = fields(&path) {
            return Some(app(&fields, &dirs));
        }
    }
    let wanted = app_id.to_lowercase();
    for dir in &dirs {
        let Ok(read) = std::fs::read_dir(dir.join("applications")) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "desktop")
            {
                continue;
            }
            if let Some(fields) = fields(&path)
                && fields
                    .get("StartupWMClass")
                    .is_some_and(|class| class.to_lowercase() == wanted)
            {
                return Some(app(&fields, &dirs));
            }
        }
    }
    None
}

/// The `[Desktop Entry]` group's keys, the first of each.
fn fields(path: &Path) -> Option<HashMap<String, String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut fields = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            fields
                .entry(key.trim().to_owned())
                .or_insert_with(|| value.trim().to_owned());
        }
    }
    Some(fields)
}

fn app(fields: &HashMap<String, String>, dirs: &[PathBuf]) -> App {
    let name = match language() {
        Language::Chinese => ["Name[zh_CN]", "Name[zh]", "Name"]
            .into_iter()
            .find_map(|key| fields.get(key)),
        Language::English => fields.get("Name"),
    };
    App {
        name: name.cloned().unwrap_or_default(),
        icon: fields
            .get("Icon")
            .filter(|icon| !icon.is_empty())
            .and_then(|icon| icon_path(icon, dirs)),
    }
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

/// `icon` in a theme, the sharpest at the size the dialog draws it: a picture
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
