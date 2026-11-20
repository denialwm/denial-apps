//! Read and write the cursor catalog shared by Denial's shell and Settings.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub const DEFAULT: &str = "bibata_modern_ice";
const MIB: usize = 1024 * 1024;
/// Each role with the value name Windows stores it under in the registry, and the
/// [Strings] keys install.inf files commonly give it.
const ROLES: &[(&str, &str, &[&str])] = &[
    ("normal", "arrow", &["pointer"]),
    ("help", "help", &["help"]),
    ("working", "appstarting", &["working", "work"]),
    ("busy", "wait", &["busy"]),
    ("precision", "crosshair", &["precision", "cross"]),
    ("text", "ibeam", &["text"]),
    ("handwriting", "nwpen", &["hand", "handwriting"]),
    ("unavailable", "no", &["unavailable"]),
    ("verticalResize", "sizens", &["vert"]),
    ("horizontalResize", "sizewe", &["horz"]),
    ("diagonalNwSeResize", "sizenwse", &["dgn1"]),
    ("diagonalNeSwResize", "sizenesw", &["dgn2"]),
    ("move", "sizeall", &["move"]),
    ("alternate", "uparrow", &["alternate"]),
    ("link", "hand", &["link"]),
    ("person", "person", &["person"]),
    ("pin", "pin", &["pin"]),
];

/// The roles a theme's card shows, as the shell names them.
pub const PREVIEW_ROLES: [&str; 5] = ["normal", "link", "text", "working", "busy"];
/// Bundled Bibata's frames in the shell's bundle: a 32 px image per role,
/// and larger renderings beside it.
const BUNDLED_FRAMES: &str = "packages/denial_flutter_sdk/assets/cursors/bibata_modern_ice";

/// One image of a cursor and how long it shows, in microseconds.
#[derive(Clone)]
pub struct PreviewFrame {
    pub path: PathBuf,
    pub duration: u64,
}

#[derive(Clone)]
pub struct Theme {
    pub id: String,
    pub label: String,
    /// The frames of each of `PREVIEW_ROLES`; a role the theme lacks shows
    /// its normal cursor, as the shell draws it. Empty when none were found.
    pub previews: Vec<Vec<PreviewFrame>>,
}

impl Theme {
    pub fn imported(&self) -> bool {
        self.id != DEFAULT
    }
}

fn bundled() -> Theme {
    let still = |role: &str| {
        ["96/00.png", "00.png"]
            .iter()
            .find_map(|file| {
                crate::wallpaper::shell_asset(&format!("{BUNDLED_FRAMES}/{role}/{file}"))
            })
            .map(|path| {
                vec![PreviewFrame {
                    path,
                    duration: STATIC_FRAME_MICROSECONDS,
                }]
            })
    };
    Theme {
        id: DEFAULT.to_owned(),
        label: "Bibata Modern Ice".to_owned(),
        previews: PREVIEW_ROLES
            .iter()
            .map(|role| still(role).or_else(|| still("normal")))
            .collect::<Option<_>>()
            .unwrap_or_default(),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn root() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".local/share"))
        })
        .map(|root| root.join("denial/cursors"))
}

fn digest_valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
}

fn manifest(directory: &Path, digest: &str) -> Option<Theme> {
    if !digest_valid(digest) || !directory.symlink_metadata().ok()?.is_dir() {
        return None;
    }
    let file = directory.join("theme.json");
    let metadata = file.symlink_metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MIB as u64 {
        return None;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let id = format!("imported:{digest}");
    let label = value["label"].as_str()?.trim();
    if value["version"] != 1
        || value["id"] != id
        || label.is_empty()
        || !value["roles"]["normal"]["frames"]
            .as_array()
            .is_some_and(|frames| !frames.is_empty())
    {
        return None;
    }
    let frames = |role: &str| -> Option<Vec<PreviewFrame>> {
        value["roles"][role]["frames"]
            .as_array()?
            .iter()
            .map(|frame| {
                let path = frame["path"].as_str()?;
                let safe = Path::new(path)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)));
                Some(PreviewFrame {
                    path: safe.then(|| directory.join(path))?,
                    duration: frame["durationMicroseconds"]
                        .as_u64()
                        .filter(|&us| us > 0)?,
                })
            })
            .collect()
    };
    Some(Theme {
        id,
        label: label.to_owned(),
        previews: PREVIEW_ROLES
            .iter()
            .map(|role| frames(role).or_else(|| frames("normal")))
            .collect::<Option<_>>()
            .unwrap_or_default(),
    })
}

pub fn discovered() -> Vec<Theme> {
    let mut imported: Vec<_> = root()
        .filter(|root| root.symlink_metadata().is_ok_and(|info| info.is_dir()))
        .and_then(|root| std::fs::read_dir(root).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| manifest(&entry.path(), entry.file_name().to_str()?))
        .collect();
    imported.sort_by_cached_key(|theme| (theme.label.to_lowercase(), theme.id.clone()));
    imported.insert(0, bundled());
    imported
}

pub fn remove(id: &str) -> io::Result<()> {
    let digest = id
        .strip_prefix("imported:")
        .filter(|digest| digest_valid(digest))
        .ok_or_else(|| invalid("Invalid imported theme"))?;
    let root = root().ok_or_else(|| invalid("Cursor catalog unavailable"))?;
    if !root.symlink_metadata()?.is_dir() {
        return Err(invalid("Invalid cursor catalog directory"));
    }
    let folder = root.join(digest);
    if manifest(&folder, digest).is_none() {
        return Err(invalid("Invalid imported theme"));
    }
    std::fs::remove_dir_all(folder)
}

fn limited_output(command: &mut Command, maximum: usize) -> io::Result<Vec<u8>> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(maximum as u64 + 1)
        .read_to_end(&mut output);
    if read.is_err() || output.len() > maximum {
        let _ = child.kill();
    }
    let status = child.wait()?;
    read?;
    if !status.success() || output.len() > maximum {
        return Err(invalid("Invalid or oversized cursor ZIP entry"));
    }
    Ok(output)
}

fn extract_entry(archive: &Path, name: &str, maximum: usize) -> io::Result<Vec<u8>> {
    limited_output(
        Command::new("bsdtar")
            .arg("-xOf")
            .arg(archive)
            .args(["--", name]),
        maximum,
    )
}

fn invalid_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Windows writes install.inf as UTF-8, ANSI or UTF-16; older tools favour UTF-16 LE.
fn inf_text(bytes: &[u8]) -> String {
    let utf16 = |body: &[u8], from: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|pair| from([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xff, 0xfe, body @ ..] => utf16(body, u16::from_le_bytes),
        [0xfe, 0xff, body @ ..] => utf16(body, u16::from_be_bytes),
        [first, 0, ..] if *first != 0 => utf16(bytes, u16::from_le_bytes),
        _ => String::from_utf8_lossy(bytes)
            .trim_start_matches('\u{feff}')
            .to_owned(),
    }
}

/// A cursor scheme an install.inf describes: its name and each role's file name, lowercase.
struct Scheme {
    label: String,
    roles: BTreeMap<&'static str, String>,
}

/// Splits an INF line at commas outside quotes, trimming each field and its quotes.
fn inf_fields(line: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (at, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(&line[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    fields.push(&line[start..]);
    fields
        .into_iter()
        .map(|field| field.trim().trim_matches('"'))
        .collect()
}

fn parse_inf(bytes: &[u8], fallback_label: &str) -> io::Result<Scheme> {
    let not_scheme = || invalid("install.inf does not describe a Windows cursor scheme");
    let text = inf_text(bytes);
    let mut strings = HashMap::new();
    // Registry value name → data, from the AddReg lines that set the user's cursors.
    let mut registry = HashMap::new();
    let mut section = "";
    for line in text.lines().map(str::trim) {
        if line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
        } else if section.trim().eq_ignore_ascii_case("strings") {
            if let Some((key, value)) = line.split_once('=')
                && strings
                    .insert(
                        key.trim().to_ascii_lowercase(),
                        value.trim().trim_matches('"').trim(),
                    )
                    .is_some()
            {
                return Err(invalid("install.inf names a cursor role twice"));
            }
        } else if let [root, key, name, _, data, ..] = inf_fields(line)[..]
            && root.eq_ignore_ascii_case("hkcu")
            && key.eq_ignore_ascii_case("control panel\\cursors")
        {
            registry.entry(name.to_ascii_lowercase()).or_insert(data);
        }
    }
    // %key% refers to [Strings]; unknown ones such as %10% (the Windows folder) drop out.
    let expand = |value: &str| -> String {
        value
            .split('%')
            .enumerate()
            .map(|(index, part)| match index % 2 {
                0 => part,
                _ => strings
                    .get(&part.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(""),
            })
            .collect()
    };
    let label = strings
        .get("scheme_name")
        .copied()
        .filter(|label| !label.is_empty())
        .unwrap_or(fallback_label);
    if label.is_empty() || label.chars().any(char::is_control) {
        return Err(not_scheme());
    }
    // Windows reads the registry lines; [Strings] keys are only a convention, so they are
    // the fallback for files without those lines.
    let by_registry = registry.contains_key("arrow");
    let mut roles = BTreeMap::new();
    for &(role, name, keys) in ROLES {
        let value = if by_registry {
            registry.get(name).map(|data| expand(data))
        } else {
            keys.iter()
                .find_map(|key| strings.get(*key))
                .map(|value| value.to_string())
        };
        let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let file = value
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !(file.ends_with(".cur") || file.ends_with(".ani")) {
            return Err(invalid_owned(format!(
                "install.inf gives the {role} cursor {value}, which is not a .cur or .ani file"
            )));
        }
        roles.insert(role, file);
    }
    if !roles.contains_key("normal") {
        return Err(not_scheme());
    }
    Ok(Scheme {
        label: label.to_owned(),
        roles,
    })
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize)
}

fn chunks(mut data: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    let mut result = Vec::new();
    while !data.is_empty() {
        let length = u32_at(data, 4)?;
        let end = 8_usize.checked_add(length)?;
        result.push((data.get(..4)?, data.get(8..end)?));
        data = data.get(end.checked_add(length % 2)?..)?;
        if result.len() > 4096 {
            return None;
        }
    }
    Some(result)
}

struct Frame {
    image: image::DynamicImage,
    size: (u32, u32),
    hotspot: (u16, u16),
}

fn cursor_frame(bytes: &[u8]) -> Option<Frame> {
    if u16_at(bytes, 0)? != 0 || u16_at(bytes, 2)? != 2 {
        return None;
    }
    let count = usize::from(u16_at(bytes, 4)?);
    if !(1..=256).contains(&count) {
        return None;
    }
    let directory = bytes.get(6..6 + 16 * count)?;
    let dimension = |byte: u8| if byte == 0 { 256 } else { u32::from(byte) };
    let entry = directory
        .chunks_exact(16)
        .max_by_key(|entry| dimension(entry[0]) * dimension(entry[1]))?;
    let size = (dimension(entry[0]), dimension(entry[1]));
    let hotspot = (u16_at(entry, 4)?, u16_at(entry, 6)?);
    let offset = u32_at(entry, 12)?;
    let length = u32_at(entry, 8)?;
    if u32::from(hotspot.0) >= size.0 || u32::from(hotspot.1) >= size.1 || offset < 6 + 16 * count {
        return None;
    }
    let payload = bytes.get(offset..offset.checked_add(length)?)?;
    let mut icon = vec![0, 0, 1, 0, 1, 0];
    icon.extend_from_slice(entry);
    icon[10..12].copy_from_slice(&1_u16.to_le_bytes());
    let bits = if payload.starts_with(b"\x89PNG\r\n\x1a\n") {
        32
    } else {
        u16_at(payload, 14)?
    };
    icon[12..14].copy_from_slice(&bits.to_le_bytes());
    icon[18..22].copy_from_slice(&22_u32.to_le_bytes());
    icon.extend_from_slice(payload);
    let mut reader = image::ImageReader::with_format(Cursor::new(icon), image::ImageFormat::Ico);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(4 * MIB as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    if (image.width(), image.height()) != size {
        return None;
    }
    Some(Frame {
        image,
        size,
        hotspot,
    })
}

struct Animation<'a> {
    images: Vec<&'a [u8]>,
    steps: Vec<(usize, u64)>,
}

fn animation(bytes: &[u8]) -> Option<Animation<'_>> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"ACON" {
        return None;
    }
    let mut header = None;
    let mut rates = None;
    let mut sequence = None;
    let mut images = Vec::new();
    for (tag, data) in chunks(bytes.get(12..8_usize.checked_add(u32_at(bytes, 4)?)?)?)? {
        match tag {
            b"anih" => {
                if header.replace(data).is_some() {
                    return None;
                }
            }
            b"rate" => {
                if rates.replace(data).is_some() {
                    return None;
                }
            }
            b"seq " => {
                if sequence.replace(data).is_some() {
                    return None;
                }
            }
            b"LIST" if data.starts_with(b"fram") => {
                images.extend(
                    chunks(&data[4..])?
                        .into_iter()
                        .filter_map(|(tag, frame)| (tag == b"icon").then_some(frame)),
                );
            }
            _ => {}
        }
    }
    let header = header?;
    let frames = u32_at(header, 4)?;
    let steps = u32_at(header, 8)?;
    if u32_at(header, 0)? < 36
        || u32_at(header, 32)? & 1 == 0
        || images.len() != frames
        || !(1..=512).contains(&frames)
        || !(1..=512).contains(&steps)
        || sequence.is_some_and(|table| table.len() != steps * 4)
        || rates.is_some_and(|table| table.len() != steps * 4)
    {
        return None;
    }
    let default_rate = u32_at(header, 28)?;
    let steps = (0..steps)
        .map(|index| {
            let image = sequence.map_or(Some(index), |table| u32_at(table, index * 4))?;
            let rate = rates.map_or(Some(default_rate), |table| u32_at(table, index * 4))?;
            (image < frames && (1..=3600).contains(&rate))
                .then(|| (image, (rate as u64 * 1_000_000 + 30) / 60))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Animation { images, steps })
}

/// A static cursor never advances, so its one frame's duration only has to be valid.
const STATIC_FRAME_MICROSECONDS: u64 = 1_000_000;

fn is_static_cursor(bytes: &[u8]) -> bool {
    u16_at(bytes, 0) == Some(0) && u16_at(bytes, 2) == Some(2)
}

/// A file's frames, told apart by content: install.inf extensions are not always right.
fn frames_of(bytes: &[u8]) -> Option<Animation<'_>> {
    if bytes.starts_with(b"RIFF") {
        animation(bytes)
    } else if is_static_cursor(bytes) {
        Some(Animation {
            images: vec![bytes],
            steps: vec![(0, STATIC_FRAME_MICROSECONDS)],
        })
    } else {
        None
    }
}

/// The longest edge of the largest image in a cursor's first frame, read from its directory.
fn extent(bytes: &[u8]) -> Option<u32> {
    let first = *frames_of(bytes)?.images.first()?;
    let count = usize::from(u16_at(first, 4)?);
    let dimension = |byte: u8| if byte == 0 { 256 } else { u32::from(byte) };
    first
        .get(6..6 + 16 * count)?
        .chunks_exact(16)
        .map(|entry| dimension(entry[0]).max(dimension(entry[1])))
        .max()
}

fn write_role(
    folder: &Path,
    role: &str,
    file: &str,
    data: &[u8],
    remaining: &mut usize,
) -> io::Result<Value> {
    let animation = frames_of(data)
        .ok_or_else(|| invalid_owned(format!("{file} ({role}) is not a valid .cur or .ani")))?;
    std::fs::create_dir(folder.join(role))?;
    let mut frames = Vec::new();
    let mut size = None;
    for (index, bytes) in animation.images.into_iter().enumerate() {
        let frame = cursor_frame(bytes).ok_or_else(|| {
            invalid_owned(format!("{file} ({role}) has an image that cannot be read"))
        })?;
        if size
            .replace(frame.size)
            .is_some_and(|previous| previous != frame.size)
        {
            return Err(invalid_owned(format!(
                "{file} ({role}) has frames of different sizes"
            )));
        }
        let path = format!("{role}/{index:03}.png");
        let mut encoded = Cursor::new(Vec::new());
        frame
            .image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(io::Error::other)?;
        let bytes = encoded.into_inner();
        *remaining = remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| invalid("Cursor theme is too large"))?;
        std::fs::write(folder.join(&path), bytes)?;
        frames.push((path, frame.hotspot));
    }
    let (width, height) = size.ok_or_else(|| invalid("No cursor frames"))?;
    let steps: Vec<_> = animation
        .steps
        .into_iter()
        .map(|(index, duration)| {
            let (path, (x, y)) = &frames[index];
            json!({"path": path, "hotspot": {"x": x, "y": y}, "durationMicroseconds": duration})
        })
        .collect();
    Ok(json!({"size": {"width": width, "height": height}, "frames": steps}))
}

fn convert(folder: &Path, archive: &Path, root: &Path) -> io::Result<Theme> {
    let digest = limited_output(Command::new("sha256sum").arg(archive), 8192)?;
    let digest = std::str::from_utf8(&digest)
        .ok()
        .and_then(|text| text.split_whitespace().next())
        .filter(|digest| digest_valid(digest))
        .ok_or_else(|| invalid("Could not identify cursor ZIP"))?;
    let destination = root.join(digest);
    if let Some(theme) = manifest(&destination, digest) {
        return Ok(theme);
    }
    // Only read entries by name into bounded buffers. Archive paths are never extracted to disk.
    let listing = limited_output(Command::new("bsdtar").arg("-tf").arg(archive), MIB)?;
    let names = std::str::from_utf8(&listing).map_err(io::Error::other)?;
    let mut seen = std::collections::HashSet::new();
    // (folder, lowercase file name, archive path) of every .inf, .cur and .ani.
    let mut entries = Vec::new();
    for (index, path) in names.lines().enumerate() {
        if index >= 512
            || path.starts_with('/')
            || path
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '*' | '?' | '[' | ']' | '\\'))
            || path.split('/').any(|part| part == "..")
        {
            return Err(invalid("Invalid cursor ZIP paths"));
        }
        if !seen.insert(path.to_lowercase()) {
            return Err(invalid("Duplicate cursor ZIP entry"));
        }
        let (parent, file) = path.rsplit_once('/').unwrap_or(("", path));
        let file = file.to_ascii_lowercase();
        if [".inf", ".cur", ".ani"]
            .iter()
            .any(|ext| file.ends_with(ext))
        {
            entries.push((parent, file, path));
        }
    }
    // A file is looked up beside its install.inf first, then anywhere when its name is unique.
    let find = |folder: &str, file: &str| {
        entries
            .iter()
            .find(|(parent, name, _)| *parent == folder && name == file)
            .or_else(|| {
                let mut named = entries.iter().filter(|(_, name, _)| name == file);
                named.next().filter(|_| named.next().is_none())
            })
            .map(|(_, _, path)| *path)
    };
    let mut remaining_input = 128 * MIB;
    let mut read = |path: &str| -> io::Result<Vec<u8>> {
        let data = extract_entry(archive, path, remaining_input.min(32 * MIB))?;
        remaining_input -= data.len();
        Ok(data)
    };
    // Packs ship one install.inf per size or variant. Denial resamples a cursor to the size it
    // is drawn at, so the scheme with the largest normal cursor gives the sharpest result.
    let mut chosen: Option<(u32, &str, Scheme)> = None;
    let mut first_error = None;
    for &(folder, ref file, path) in &entries {
        if !file.ends_with(".inf") {
            continue;
        }
        let fallback = folder.rsplit('/').next().unwrap_or("");
        let scheme = read(path).and_then(|bytes| parse_inf(&bytes, fallback));
        let measured = scheme.and_then(|scheme| {
            let normal = &scheme.roles["normal"];
            let source = find(folder, normal).ok_or_else(|| {
                invalid_owned(format!("{normal}, the normal cursor in {path}, is missing"))
            })?;
            let size = extent(&read(source)?).ok_or_else(|| {
                invalid_owned(format!("{normal} (normal) is not a valid .cur or .ani"))
            })?;
            Ok((size, scheme))
        });
        match measured {
            Ok((size, scheme)) if chosen.as_ref().is_none_or(|(best, ..)| size > *best) => {
                chosen = Some((size, folder, scheme));
            }
            Ok(_) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    let Some((_, folder_of_inf, scheme)) = chosen else {
        return Err(first_error.unwrap_or_else(|| invalid("Cursor ZIP must contain install.inf")));
    };
    let mut remaining_output = 128 * MIB;
    let mut role_values = BTreeMap::new();
    for (role, file) in &scheme.roles {
        let path = find(folder_of_inf, file).ok_or_else(|| {
            invalid_owned(format!(
                "{file}, the {role} cursor in install.inf, is missing"
            ))
        })?;
        let data = read(path)?;
        role_values.insert(
            *role,
            write_role(folder, role, file, &data, &mut remaining_output)?,
        );
    }
    let id = format!("imported:{digest}");
    let label = scheme.label;
    std::fs::write(
        folder.join("theme.json"),
        serde_json::to_vec(&json!({
            "version": 1, "id": id, "label": label, "author": "", "sourceFormat": "windows",
            "sourceArchiveSha256": digest, "roles": role_values
        }))?,
    )?;
    std::fs::remove_file(archive)?;
    match std::fs::rename(folder, &destination) {
        Ok(()) => {
            manifest(&destination, digest).ok_or_else(|| invalid("Imported theme unreadable"))
        }
        Err(error) => manifest(&destination, digest).ok_or(error),
    }
}

pub fn import(source: &Path) -> io::Result<Theme> {
    if !source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        || !source.symlink_metadata()?.is_file()
    {
        return Err(invalid("Choose a regular cursor ZIP file"));
    }
    let root = root().ok_or_else(|| invalid("Cursor catalog unavailable"))?;
    std::fs::create_dir_all(&root)?;
    if !root.symlink_metadata()?.is_dir() {
        return Err(invalid("Invalid cursor catalog directory"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let folder = root.join(format!(".import-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&folder)?;
    let archive = folder.join("source.zip");
    let result = (|| {
        let mut source = std::fs::File::open(source)?;
        let mut signature = [0; 4];
        source.read_exact(&mut signature)?;
        if &signature != b"PK\x03\x04" {
            return Err(invalid("Not a cursor ZIP archive"));
        }
        let mut target = std::fs::File::create(&archive)?;
        let input = io::Cursor::new(signature).chain(source);
        let count = io::copy(&mut input.take((64 * MIB + 1) as u64), &mut target)?;
        if count == 0 || count > (64 * MIB) as u64 {
            return Err(invalid("Cursor ZIP is empty or too large"));
        }
        convert(&folder, &archive, &root)
    })();
    if folder.exists() {
        let _ = std::fs::remove_dir_all(folder);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
        let mut chunk = tag.to_vec();
        chunk.extend((bytes.len() as u32).to_le_bytes());
        chunk.extend(bytes);
        if bytes.len() % 2 != 0 {
            chunk.push(0);
        }
        chunk
    }

    /// A one-image .cur of `side` pixels with its hotspot at (1, 1).
    fn cur(side: u8) -> Vec<u8> {
        let mut png = Cursor::new(Vec::new());
        let image = image::DynamicImage::new_rgba8(side.into(), side.into());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let mut cur = vec![0, 0, 2, 0, 1, 0, side, side, 0, 0, 1, 0, 1, 0];
        cur.extend((png.len() as u32).to_le_bytes());
        cur.extend(22_u32.to_le_bytes());
        cur.extend(png);
        cur
    }

    fn ani(side: u8) -> Vec<u8> {
        let header: Vec<_> = [36_u32, 1, 1, 0, 0, 0, 0, 6, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        chunk(
            b"RIFF",
            &[
                b"ACON".as_slice(),
                &chunk(b"anih", &header),
                &chunk(
                    b"LIST",
                    &[b"fram".as_slice(), &chunk(b"icon", &cur(side))].concat(),
                ),
            ]
            .concat(),
        )
    }

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Zips `files` and imports the archive into a fresh catalog; None without bsdtar.
    fn import_files(name: &str, files: &[(&str, Vec<u8>)]) -> Option<(io::Result<Value>, Cleanup)> {
        let folder =
            std::env::temp_dir().join(format!("denial-cursor-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        let cleanup = Cleanup(folder.clone());
        let fixture = folder.join("fixture");
        let staging = folder.join("staging");
        std::fs::create_dir(&staging).unwrap();
        for (path, bytes) in files {
            let path = fixture.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let archive = staging.join("source.zip");
        let status = Command::new("bsdtar")
            .args(["--format", "zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&fixture)
            .args(files.iter().map(|(path, _)| path))
            .status();
        if status
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            return None;
        }
        assert!(status.unwrap().success());
        let result = convert(&staging, &archive, &folder).map(|imported| {
            let destination = folder.join(imported.id.strip_prefix("imported:").unwrap());
            let mut manifest: Value =
                serde_json::from_slice(&std::fs::read(destination.join("theme.json")).unwrap())
                    .unwrap();
            for (role, value) in manifest["roles"].as_object().unwrap() {
                for frame in value["frames"].as_array().unwrap() {
                    let path = frame["path"].as_str().unwrap();
                    assert!(destination.join(path).is_file(), "{role}: {path}");
                }
            }
            manifest["importedLabel"] = json!(imported.label);
            manifest
        });
        Some((result, cleanup))
    }

    #[test]
    fn animated_scheme_converts_to_denial_manifest() {
        let Some((manifest, _cleanup)) = import_files(
            "animated",
            &[
                (
                    "install.inf",
                    b"[Strings]\nscheme_name=Demo\npointer=pointer.ani\n".to_vec(),
                ),
                ("pointer.ani", ani(2)),
            ],
        ) else {
            return;
        };
        let manifest = manifest.unwrap();
        assert_eq!(manifest["importedLabel"], "Demo");
        let frame = &manifest["roles"]["normal"]["frames"][0];
        assert_eq!(frame["durationMicroseconds"], 100_000);
        assert_eq!(frame["hotspot"]["x"], 1);
    }

    #[test]
    fn static_roles_and_utf16_inf_are_read() {
        let inf = "[Strings]\r\nSCHEME_NAME = \"Still\"\r\npointer = \"Pointer.cur\"\r\nbusy = \"Busy.ani\"\r\n";
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(inf.encode_utf16().flat_map(u16::to_le_bytes));
        let Some((manifest, _cleanup)) = import_files(
            "static",
            &[
                ("Still/install.inf", utf16),
                ("Still/Pointer.cur", cur(4)),
                ("Still/Busy.ani", ani(4)),
            ],
        ) else {
            return;
        };
        let manifest = manifest.unwrap();
        assert_eq!(manifest["importedLabel"], "Still");
        let normal = &manifest["roles"]["normal"];
        assert_eq!(normal["size"]["width"], 4);
        assert_eq!(normal["frames"].as_array().unwrap().len(), 1);
        assert_eq!(
            normal["frames"][0]["durationMicroseconds"],
            STATIC_FRAME_MICROSECONDS
        );
        assert_eq!(
            manifest["roles"]["busy"]["frames"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn largest_variant_of_a_multi_scheme_pack_is_imported() {
        let inf = |name: &str| {
            format!("[Strings]\nscheme_name={name}\npointer=Pointer.cur\ntext=Text.cur\n")
                .into_bytes()
        };
        let Some((manifest, _cleanup)) = import_files(
            "variants",
            &[
                ("Pack-Regular/install.inf", inf("Pack Regular")),
                ("Pack-Regular/Pointer.cur", cur(8)),
                ("Pack-Regular/Text.cur", cur(8)),
                ("Pack-Extra-Large/install.inf", inf("Pack Extra Large")),
                ("Pack-Extra-Large/Pointer.cur", cur(24)),
                ("Pack-Extra-Large/Text.cur", cur(24)),
                ("Pack-Large/install.inf", inf("Pack Large")),
                ("Pack-Large/Pointer.cur", cur(16)),
                ("Pack-Large/Text.cur", cur(16)),
            ],
        ) else {
            return;
        };
        let manifest = manifest.unwrap();
        assert_eq!(manifest["importedLabel"], "Pack Extra Large");
        assert_eq!(manifest["roles"]["normal"]["size"]["width"], 24);
        assert_eq!(manifest["roles"]["text"]["size"]["width"], 24);
    }

    #[test]
    fn a_role_that_is_not_a_cursor_is_named() {
        let Some((manifest, _cleanup)) = import_files(
            "named",
            &[
                (
                    "install.inf",
                    b"[Strings]\nscheme_name=Bad\npointer=pointer.png\n".to_vec(),
                ),
                ("pointer.cur", cur(2)),
            ],
        ) else {
            return;
        };
        let error = manifest.unwrap_err().to_string();
        assert!(error.contains("pointer.png"), "{error}");
    }

    #[test]
    fn roles_come_from_the_registry_lines() {
        let inf = concat!(
            "[Wreg]\r\n",
            "HKCU,\"Control Panel\\Cursors\",Arrow,0x00020000,\"%10%\\%CUR_DIR%\\%pointer%\"\r\n",
            "HKCU,\"Control Panel\\Cursors\",AppStarting,0x00020000,\"%10%\\%CUR_DIR%\\%work%\"\r\n",
            "HKCU,\"Control Panel\\Cursors\",crosshair,0x00020000,\"%10%\\%CUR_DIR%\\%cross%\"\r\n",
            "HKCU,\"Control Panel\\Cursors\",NWPen,0x00020000,\"%10%\\%CUR_DIR%\\%handwriting%\"\r\n",
            "[Strings]\r\n",
            "CUR_DIR = \"Cursors\\Demo\"\r\n",
            "SCHEME_NAME = \"Demo\"\r\n",
            "pointer = \"Pointer.cur\"\r\n",
            "work = \"Work.ani\"\r\n",
            "cross = \"Cross.cur\"\r\n",
            "handwriting = \"Handwriting.cur\"\r\n",
        );
        let scheme = parse_inf(inf.as_bytes(), "").unwrap();
        assert_eq!(scheme.label, "Demo");
        assert_eq!(scheme.roles["normal"], "pointer.cur");
        assert_eq!(scheme.roles["working"], "work.ani");
        assert_eq!(scheme.roles["precision"], "cross.cur");
        assert_eq!(scheme.roles["handwriting"], "handwriting.cur");
    }
}
