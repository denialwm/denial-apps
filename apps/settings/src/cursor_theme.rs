//! Read and write the cursor catalog shared by Denial's shell and Settings.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub const DEFAULT: &str = "bibata_modern_ice";
const MIB: usize = 1024 * 1024;
const ROLE_KEYS: &[(&str, &str)] = &[
    ("normal", "pointer"),
    ("help", "help"),
    ("working", "working"),
    ("busy", "busy"),
    ("precision", "precision"),
    ("text", "text"),
    ("handwriting", "hand"),
    ("unavailable", "unavailable"),
    ("verticalResize", "vert"),
    ("horizontalResize", "horz"),
    ("diagonalNwSeResize", "dgn1"),
    ("diagonalNeSwResize", "dgn2"),
    ("move", "move"),
    ("alternate", "alternate"),
    ("link", "link"),
    ("person", "person"),
    ("pin", "pin"),
];

#[derive(Clone)]
pub struct Theme {
    pub id: String,
    pub label: String,
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
    Some(Theme {
        id,
        label: label.to_owned(),
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
    imported.insert(
        0,
        Theme {
            id: DEFAULT.to_owned(),
            label: "Bibata Modern Ice".to_owned(),
        },
    );
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

fn parse_inf(bytes: &[u8]) -> Option<(String, BTreeMap<&'static str, String>)> {
    let text = String::from_utf8_lossy(bytes);
    let mut strings = HashMap::new();
    let mut section = "";
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
        } else if section.trim().eq_ignore_ascii_case("strings")
            && !line.starts_with(';')
            && let Some((key, value)) = line.split_once('=')
            && strings
                .insert(
                    key.trim().to_ascii_lowercase(),
                    value.trim().trim_matches('"').trim(),
                )
                .is_some()
        {
            return None;
        }
    }
    let label = strings
        .get("scheme_name")
        .filter(|label| !label.is_empty() && !label.chars().any(char::is_control))?;
    let mut roles = BTreeMap::new();
    for &(role, key) in ROLE_KEYS {
        if let Some(value) = strings.get(key).filter(|value| !value.is_empty()) {
            let file = value.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
            if !file.ends_with(".ani") {
                return None;
            }
            roles.insert(role, file);
        }
    }
    roles
        .contains_key("normal")
        .then(|| (label.to_string(), roles))
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

fn write_role(folder: &Path, role: &str, data: &[u8], remaining: &mut usize) -> io::Result<Value> {
    let animation = animation(data).ok_or_else(|| invalid("Invalid animated cursor"))?;
    std::fs::create_dir(folder.join(role))?;
    let mut frames = Vec::new();
    let mut size = None;
    for (index, bytes) in animation.images.into_iter().enumerate() {
        let frame = cursor_frame(bytes).ok_or_else(|| invalid("Invalid animated cursor frame"))?;
        if size
            .replace(frame.size)
            .is_some_and(|previous| previous != frame.size)
        {
            return Err(invalid("Cursor frames have different sizes"));
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
    let mut paths = HashMap::new();
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
        let file = path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        if (file == "install.inf" || file.ends_with(".ani")) && paths.insert(file, path).is_some() {
            return Err(invalid("Duplicate cursor ZIP entry"));
        }
    }
    let inf = paths
        .get("install.inf")
        .ok_or_else(|| invalid("Cursor ZIP must contain install.inf"))?;
    let (label, roles) = parse_inf(&extract_entry(archive, inf, MIB)?)
        .ok_or_else(|| invalid("Invalid cursor scheme"))?;
    let mut remaining_input = 128 * MIB;
    let mut remaining_output = 128 * MIB;
    let mut role_values = BTreeMap::new();
    for (role, file) in roles {
        let path = paths
            .get(&file)
            .ok_or_else(|| invalid("Cursor referenced in install.inf is missing"))?;
        let data = extract_entry(archive, path, remaining_input.min(32 * MIB))?;
        remaining_input -= data.len();
        role_values.insert(
            role,
            write_role(folder, role, &data, &mut remaining_output)?,
        );
    }
    let id = format!("imported:{digest}");
    std::fs::write(
        folder.join("theme.json"),
        serde_json::to_vec(&json!({
            "version": 1, "id": id, "label": label, "author": "", "sourceFormat": "windows-ani",
            "sourceArchiveSha256": digest, "roles": role_values
        }))?,
    )?;
    std::fs::remove_file(archive)?;
    match std::fs::rename(folder, &destination) {
        Ok(()) => Ok(Theme { id, label }),
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

    #[test]
    fn cursor_zip_converts_to_denial_manifest() {
        let folder =
            std::env::temp_dir().join(format!("denial-cursor-test-{}", std::process::id()));
        std::fs::create_dir(&folder).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(folder.clone());
        let fixture = folder.join("fixture");
        let staging = folder.join("staging");
        std::fs::create_dir(&fixture).unwrap();
        std::fs::create_dir(&staging).unwrap();
        let mut png = Cursor::new(Vec::new());
        let image = image::DynamicImage::new_rgba8(2, 2);
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let mut cur = vec![0, 0, 2, 0, 1, 0, 2, 2, 0, 0, 1, 0, 1, 0];
        cur.extend((png.len() as u32).to_le_bytes());
        cur.extend(22_u32.to_le_bytes());
        cur.extend(png);
        let header: Vec<_> = [36_u32, 1, 1, 0, 0, 0, 0, 6, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let ani = chunk(
            b"RIFF",
            &[
                b"ACON".as_slice(),
                &chunk(b"anih", &header),
                &chunk(
                    b"LIST",
                    &[b"fram".as_slice(), &chunk(b"icon", &cur)].concat(),
                ),
            ]
            .concat(),
        );
        std::fs::write(fixture.join("pointer.ani"), ani).unwrap();
        std::fs::write(
            fixture.join("install.inf"),
            "[Strings]\nscheme_name=Demo\npointer=pointer.ani\n",
        )
        .unwrap();
        let archive = staging.join("source.zip");
        let status = Command::new("bsdtar")
            .args(["--format", "zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&fixture)
            .args(["install.inf", "pointer.ani"])
            .status();
        if status
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            return;
        }
        assert!(status.unwrap().success());
        let imported = convert(&staging, &archive, &folder).unwrap();
        assert_eq!(imported.label, "Demo");
        let destination = folder.join(imported.id.strip_prefix("imported:").unwrap());
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(destination.join("theme.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["roles"]["normal"]["frames"][0]["durationMicroseconds"],
            100_000
        );
        assert_eq!(manifest["roles"]["normal"]["frames"][0]["hotspot"]["x"], 1);
        assert!(destination.join("normal/000.png").is_file());
    }
}
