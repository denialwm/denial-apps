//! The wallpaper, as Denial's shell keeps it: `$XDG_STATE_HOME/denial/wallpaper`,
//! JSON naming the picture for every screen, which the shell watches and
//! shows as soon as it changes. The pictures offered are Denial's own and
//! the ones in the wallpaper folder, as the shell's picker offers them.
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
/// The placement preview's picture, uncropped: its longer side, in pixels.
const PREVIEW_SIDE: u32 = 900;

/// The largest zoom the shell takes.
pub const MAX_ZOOM: f32 = 4.0;

/// Where the wallpaper sits on the screen, as the shell draws it: scaled to
/// cover the screen, enlarged `zoom` times about the point that `x` and `y`
/// name, each from the leading edge at -1 to the trailing edge at 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Placement {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Placement {
    /// The picture's place in a `width` by `height` screen when it is
    /// `picture` pixels: its left and top edges and its size.
    pub fn frame(&self, (width, height): (f32, f32), picture: (u32, u32)) -> [f32; 4] {
        let (w, h) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
        let scale = f32::max(width / w, height / h) * self.zoom;
        let (shown_w, shown_h) = (w * scale, h * scale);
        [
            (width - shown_w) * (self.x + 1.0) / 2.0,
            (height - shown_h) * (self.y + 1.0) / 2.0,
            shown_w,
            shown_h,
        ]
    }
}

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

/// Where the wallpaper sits, as the shell's file says.
pub fn placement() -> Placement {
    let state: Option<Value> = state_file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let Some(state) = state else {
        return Placement::default();
    };
    let number = |key: &str| {
        state[key]
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|n| n as f32)
    };
    Placement {
        x: number("alignmentX").unwrap_or(0.0).clamp(-1.0, 1.0),
        y: number("alignmentY").unwrap_or(0.0).clamp(-1.0, 1.0),
        zoom: number("zoom").unwrap_or(1.0).clamp(1.0, MAX_ZOOM),
    }
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

/// The wallpaper folder, as the shell finds it: `$DENIAL_WALLPAPER_DIR`,
/// else `~/Pictures/Wallpapers`. Downloaded wallpapers go there too.
pub fn folder() -> Option<PathBuf> {
    std::env::var_os("DENIAL_WALLPAPER_DIR")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join("Pictures/Wallpapers")))
}

/// Denial's own wallpaper and the pictures in the wallpaper folder.
pub fn choices() -> Vec<Choice> {
    let mut files: Vec<PathBuf> = folder()
        .and_then(|folder| std::fs::read_dir(folder).ok())
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
    state["outputs"] = json!({});
    write_placement(&mut state, Placement::default());
    write_state(&path, &state)
}

/// Places the wallpaper on every screen: each screen's own placement goes.
pub fn place(placement: Placement) -> std::io::Result<()> {
    let path = state_file().ok_or(std::io::ErrorKind::NotFound)?;
    let mut state = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"version": 5, "all": DENIAL}));
    write_placement(&mut state, placement);
    write_state(&path, &state)
}

fn write_placement(state: &mut Value, placement: Placement) {
    // The anchors older shells read, as the shell names them.
    let anchor = |at: f32, names: [&'static str; 3]| match at {
        at if at < -0.5 => names[0],
        at if at > 0.5 => names[2],
        _ => names[1],
    };
    state["alignmentX"] = json!(placement.x);
    state["alignmentY"] = json!(placement.y);
    state["horizontalAlignment"] = json!(anchor(placement.x, ["left", "center", "right"]));
    state["verticalAlignment"] = json!(anchor(placement.y, ["top", "center", "bottom"]));
    state["zoom"] = json!(placement.zoom);
    state["outputAlignments"] = json!({});
}

/// Writes the shell's file beside itself and renames it over.
fn write_state(path: &Path, state: &Value) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(state)?)?;
    std::fs::rename(&temporary, path)
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

/// Where the cache keeps `kind` of picture of `file`, under the file's
/// name, size and time, so a changed file gets a new one.
fn cached(file: &Path, kind: &str) -> Option<PathBuf> {
    let metadata = std::fs::metadata(file).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (file, metadata.len(), metadata.modified().ok()).hash(&mut hasher);
    Some(cache()?.join(format!("{:016x}{kind}.jpg", hasher.finish())))
}

fn decode(file: &Path) -> Option<image::DynamicImage> {
    image::ImageReader::open(file)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .map_err(|error| eprintln!("settings: reading {}: {error}", file.display()))
        .ok()
}

/// Saves `picture` as `to`, through a file beside it.
fn save(picture: &image::RgbImage, to: &Path) -> Option<()> {
    std::fs::create_dir_all(to.parent()?)
        .map_err(|error| eprintln!("settings: making {}: {error}", to.display()))
        .ok()?;
    let temporary = to.with_extension("new");
    picture
        .save_with_format(&temporary, image::ImageFormat::Jpeg)
        .ok()?;
    std::fs::rename(&temporary, to).ok()
}

/// `file` whole and smaller, for the placement preview, with its size:
/// made once and kept in the cache.
pub fn preview(file: &Path) -> Option<(PathBuf, (u32, u32))> {
    let preview = cached(file, "-whole")?;
    if preview.is_file() {
        return Some((preview.clone(), image::image_dimensions(&preview).ok()?));
    }
    let picture = decode(file)?
        .thumbnail(PREVIEW_SIDE, PREVIEW_SIDE)
        .into_rgb8();
    save(&picture, &preview)?;
    Some((preview, picture.dimensions()))
}

/// A small picture of `file` in the screen's shape, made once and kept in
/// the cache.
pub fn thumbnail(file: &Path) -> Option<PathBuf> {
    let thumb = cached(file, "")?;
    if thumb.is_file() {
        return Some(thumb);
    }
    let picture = decode(file)?;
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
    save(&picture, &thumb)?;
    Some(thumb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_covers_then_zooms() {
        // A square on a 100 by 200 screen covers it 200 wide.
        let centred = Placement::default();
        assert_eq!(
            centred.frame((100.0, 200.0), (50, 50)),
            [-50.0, 0.0, 200.0, 200.0]
        );
        let left = Placement { x: -1.0, ..centred };
        assert_eq!(
            left.frame((100.0, 200.0), (50, 50)),
            [0.0, 0.0, 200.0, 200.0]
        );
        let zoomed = Placement {
            zoom: 2.0,
            y: 1.0,
            ..centred
        };
        assert_eq!(
            zoomed.frame((100.0, 200.0), (50, 50)),
            [-150.0, -200.0, 400.0, 400.0]
        );
    }

    #[test]
    fn names_come_from_files() {
        assert_eq!(name("file:/home/a/Pictures/Wallpapers/dunes.jpg"), "dunes");
        assert_eq!(name(DENIAL), "Denial");
    }
}

