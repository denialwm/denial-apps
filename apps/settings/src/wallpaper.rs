//! The wallpaper, as Denial's shell keeps it: `$XDG_STATE_HOME/denial/wallpaper`,
//! JSON naming the picture for every screen, which the shell watches and
//! shows as soon as it changes. The pictures offered are Denial's own and
//! the ones in `~/Pictures/Wallpapers`, as the shell's picker offers them.
//!
//! The lock screen sets its clock behind what stands in front, from a depth
//! mask of the wallpaper that `wallpaper-depth` makes; when it is installed,
//! a new wallpaper gets its mask here, as the shell's picker does.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::imageops::FilterType;
use serde_json::{Value, json};

/// Denial's own wallpaper, as the shell names it.
pub const DENIAL: &str =
    "asset:packages/denial_flutter_sdk/assets/wallpapers/trails-h-accent-9-dark.png";

/// Thumbnails, in pixels: a phone screen's shape.
const THUMB_WIDTH: u32 = 270;
const THUMB_HEIGHT: u32 = 585;

/// A wallpaper to pick: how the shell names it, and a picture of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub resource: String,
    pub name: String,
    /// The picture itself, or None for Denial's own when its file is not
    /// found.
    pub file: Option<PathBuf>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn state_file() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".local/state")))
        .map(|dir| dir.join("denial/wallpaper"))
}

fn cache() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".cache")))
        .map(|dir| dir.join("denial/settings/wallpapers"))
}

/// The wallpaper every screen shows, as the shell names it.
pub fn current() -> Option<String> {
    let state: Value = serde_json::from_slice(&std::fs::read(state_file()?).ok()?).ok()?;
    state["all"].as_str().map(str::to_owned)
}

/// What a resource is called in a row: its file's name, or Denial.
pub fn name(resource: &str) -> String {
    match resource.strip_prefix("file:") {
        Some(path) => Path::new(path).file_stem().map_or_else(
            || path.to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        ),
        None => "Denial".to_owned(),
    }
}

/// Denial's own wallpaper and the pictures in `~/Pictures/Wallpapers`.
pub fn choices() -> Vec<Choice> {
    let mut files: Vec<PathBuf> = home()
        .and_then(|home| std::fs::read_dir(home.join("Pictures/Wallpapers")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "jpg" | "jpeg" | "png" | "webp"
                    )
                })
        })
        .collect();
    files.sort();
    let denial = Choice {
        resource: DENIAL.to_owned(),
        name: "Denial".to_owned(),
        file: denial_file(),
    };
    std::iter::once(denial)
        .chain(files.into_iter().map(|file| {
            let resource = format!("file:{}", file.display());
            Choice {
                name: name(&resource),
                resource,
                file: Some(file),
            }
        }))
        .collect()
}

/// Denial's own wallpaper in the shell's bundle, beside the compositor that
/// started Settings, or where packages put it.
fn denial_file() -> Option<PathBuf> {
    let asset = DENIAL.strip_prefix("asset:")?;
    let beside = std::env::var_os("DENIAL_COMPOSITOR_BINARY")
        .map(PathBuf::from)
        .and_then(|binary| Some(binary.parent()?.parent()?.join("shell")));
    beside
        .into_iter()
        .chain([
            PathBuf::from("/usr/lib/denial/shell"),
            PathBuf::from("/usr/share/denial/shell"),
        ])
        .map(|bundle| bundle.join("data/flutter_assets").join(asset))
        .find(|file| file.is_file())
}

/// Makes every screen show `choice`: the shell's file is rewritten beside
/// itself and renamed over, keeping its version and dimming, and each
/// screen's own picture and placement go.
pub fn set(choice: &Choice) -> std::io::Result<()> {
    let path = state_file().ok_or(std::io::ErrorKind::NotFound)?;
    let mut state = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"version": 5}));
    state["all"] = json!(choice.resource);
    state["alignmentX"] = json!(0.0);
    state["alignmentY"] = json!(0.0);
    state["horizontalAlignment"] = json!("center");
    state["verticalAlignment"] = json!("center");
    state["outputs"] = json!({});
    state["outputAlignments"] = json!({});
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&state)?)?;
    std::fs::rename(&temporary, &path)
}

/// Starts the lock screen's depth mask of `file`, when `wallpaper-depth` is
/// installed, at the lowest priority; it runs on after Settings closes.
pub fn make_depth(file: &Path) {
    let started = Command::new("nice")
        .args(["-n", "19", "wallpaper-depth", "mask"])
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match started {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("settings: starting wallpaper-depth: {error}"),
    }
}

/// Whether `wallpaper-depth` is installed, so the lock screen has depth.
pub fn depth_installed() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|folder| folder.join("wallpaper-depth").is_file())
    })
}

/// A small picture of `file`, made once and kept in the cache under the
/// file's name, size and time, so a changed file gets a new one.
pub fn thumbnail(file: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(file).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (file, metadata.len(), metadata.modified().ok()).hash(&mut hasher);
    let thumb = cache()?.join(format!("{:016x}.jpg", hasher.finish()));
    if thumb.is_file() {
        return Some(thumb);
    }
    let picture = image::ImageReader::open(file)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .map_err(|error| eprintln!("settings: reading {}: {error}", file.display()))
        .ok()?;
    // Shrinks fast to twice the size first: wallpapers run to 40 megapixels.
    let cover = f64::max(
        f64::from(THUMB_WIDTH * 2) / f64::from(picture.width().max(1)),
        f64::from(THUMB_HEIGHT * 2) / f64::from(picture.height().max(1)),
    )
    .min(1.0);
    let picture = picture
        .thumbnail(
            (f64::from(picture.width()) * cover).ceil() as u32,
            (f64::from(picture.height()) * cover).ceil() as u32,
        )
        .resize_to_fill(THUMB_WIDTH, THUMB_HEIGHT, FilterType::Triangle)
        .into_rgb8();
    std::fs::create_dir_all(thumb.parent()?)
        .map_err(|error| eprintln!("settings: making {}: {error}", thumb.display()))
        .ok()?;
    let temporary = thumb.with_extension("new");
    picture
        .save_with_format(&temporary, image::ImageFormat::Jpeg)
        .ok()?;
    std::fs::rename(&temporary, &thumb).ok()?;
    Some(thumb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_files() {
        assert_eq!(name("file:/home/a/Pictures/Wallpapers/dunes.jpg"), "dunes");
        assert_eq!(name(DENIAL), "Denial");
    }
}

