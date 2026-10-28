//! Wallpapers from Wallhaven (wallhaven.cc), found as Denial's shell finds
//! them: safe for work, the month's toplist when nothing is asked, else the
//! best matches, at least the screen's size, the screen's shape first. A
//! picked one is downloaded into the wallpaper folder under the name the
//! shell gives it, so either finds it there. `WALLHAVEN_API_KEY`, when set,
//! goes with each search, as the shell sends it.
//!
//! Every call here blocks; Settings runs them in the background.

use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use denial_look::words::fill;
use serde_json::Value;

const SEARCH: &str = "https://wallhaven.cc/api/v1/search";
const USER_AGENT: &str = "denial-settings/1.0";
/// Words left out of every search, as the shell leaves them out.
const LEFT_OUT: [&str; 3] = ["cosplay", "cosplayer", "cosplayers"];
const MAX_SEARCH: u64 = 2 << 20;
const MAX_THUMB: u64 = 2 << 20;
const MAX_WALLPAPER: u64 = 64 << 20;

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .into()
});

/// One wallpaper Wallhaven found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub id: String,
    /// Wallhaven's small picture of it, in its own shape.
    pub thumb: String,
    /// The picture itself.
    pub full: String,
    pub width: u32,
    pub height: u32,
}

/// One page of what a search found.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub found: Vec<Found>,
    /// More pages follow this one.
    pub more: bool,
}

/// Why a call failed, in words for the screen.
fn why(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => fill(
            "Wallhaven answered HTTP {code}",
            &[("code", &code.to_string())],
        ),
        ureq::Error::Timeout(_) => "Wallhaven did not answer in time".to_owned(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            "Wallhaven is unreachable".to_owned()
        }
        ureq::Error::Io(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            "Wallhaven did not answer in time".to_owned()
        }
        ureq::Error::BodyExceedsLimit(_) => "Wallhaven sent too much".to_owned(),
        error => error.to_string(),
    }
}

/// Only Wallhaven's own addresses are fetched.
fn wallhaven(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| host == "wallhaven.cc" || host.ends_with(".wallhaven.cc"))
}

/// Page `page` (from 1) of what Wallhaven finds for `text`, for a screen of
/// `screen` pixels, wide by high.
pub fn search(text: &str, page: u32, screen: (u32, u32)) -> Result<Page, String> {
    let text = text.trim();
    let query = std::iter::once(text.to_owned())
        .chain(LEFT_OUT.iter().map(|word| format!("-{word}")))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut request = AGENT
        .get(SEARCH)
        .header("Accept", "application/json")
        .query("q", &query)
        .query("page", page.max(1).to_string())
        .query("categories", "111")
        .query("purity", "100")
        .query(
            "sorting",
            if text.is_empty() {
                "toplist"
            } else {
                "relevance"
            },
        );
    if text.is_empty() {
        request = request.query("topRange", "1M");
    }
    if screen.0 > 0 && screen.1 > 0 {
        request = request.query("atleast", format!("{}x{}", screen.0, screen.1));
    }
    if let Some(key) = std::env::var("WALLHAVEN_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
    {
        request = request.query("apikey", key.trim());
    }
    let body = request
        .call()
        .map_err(why)?
        .body_mut()
        .with_config()
        .limit(MAX_SEARCH)
        .read_to_string()
        .map_err(why)?;
    let answer: Value =
        serde_json::from_str(&body).map_err(|_| "Wallhaven's answer made no sense".to_owned())?;
    Ok(read_page(&answer, screen))
}

/// A search's answer, the ones nearest the screen's shape first.
fn read_page(answer: &Value, screen: (u32, u32)) -> Page {
    let mut found: Vec<Found> = answer["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let text = |value: &Value| value.as_str().map(str::to_owned);
            let size = |key: &str| item[key].as_u64().and_then(|n| u32::try_from(n).ok());
            let found = Found {
                id: text(&item["id"]).filter(|id| !id.is_empty())?,
                thumb: text(&item["thumbs"]["original"]).filter(|url| wallhaven(url))?,
                full: text(&item["path"]).filter(|url| wallhaven(url))?,
                width: size("dimension_x").unwrap_or(0),
                height: size("dimension_y").unwrap_or(0),
            };
            Some(found)
        })
        .collect();
    let shape = |width: u32, height: u32| {
        if width == 0 || height == 0 {
            1.0
        } else {
            f64::from(width) / f64::from(height)
        }
    };
    let target = shape(screen.0, screen.1);
    let distance = |found: &Found| (shape(found.width, found.height) / target).ln().abs();
    found.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    let meta = &answer["meta"];
    let more = match (meta["current_page"].as_u64(), meta["last_page"].as_u64()) {
        (Some(current), Some(last)) => current < last,
        _ => false,
    };
    Page { found, more }
}

fn cache() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|dir| dir.join("denial/settings/wallhaven"))
}

/// Wallhaven's small picture of `found`, fetched once and kept in the cache.
pub fn thumbnail(found: &Found) -> Result<PathBuf, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    found.thumb.hash(&mut hasher);
    let file = cache()
        .ok_or("No home folder")?
        .join(format!("{:016x}.jpg", hasher.finish()));
    if file.is_file() {
        return Ok(file);
    }
    let bytes = AGENT
        .get(&found.thumb)
        .call()
        .map_err(why)?
        .body_mut()
        .with_config()
        .limit(MAX_THUMB)
        .read_to_vec()
        .map_err(why)?;
    write_whole(&file, &bytes).map_err(|error| error.to_string())?;
    Ok(file)
}

/// Where `found` goes in `folder`: the name the shell gives it.
pub fn file_in(folder: &Path, found: &Found) -> PathBuf {
    let id: String = found
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let extension = Path::new(&found.full)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp"))
        .unwrap_or_else(|| "jpg".to_owned());
    folder.join(format!("wallhaven-{id}.{extension}"))
}

/// Downloads `found` into `folder`, unless it is there already, telling
/// `progress` the share received as it comes.
pub fn download(found: &Found, folder: &Path, progress: impl Fn(f32)) -> Result<PathBuf, String> {
    let file = file_in(folder, found);
    if file.is_file() {
        return Ok(file);
    }
    let mut response = AGENT.get(&found.full).call().map_err(why)?;
    let total = response.body().content_length().filter(|total| *total > 0);
    if total.is_some_and(|total| total > MAX_WALLPAPER) {
        return Err("The wallpaper is larger than 64 MiB".to_owned());
    }
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let partial = file.with_extension("part");
    let received = (|| {
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(MAX_WALLPAPER)
            .reader();
        let mut out = std::fs::File::create(&partial).map_err(|error| error.to_string())?;
        let mut buffer = vec![0; 64 << 10];
        let mut got = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            got += read as u64;
            if let Some(total) = total {
                progress((got as f32 / total as f32).min(1.0));
            }
        }
        out.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&partial, &file).map_err(|error| error.to_string())
    })();
    if received.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    received.map(|()| file)
}

/// Writes `bytes` beside `file` and renames them over it.
fn write_whole(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let partial = file.with_extension("part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_page() {
        let answer = json!({
            "data": [
                {
                    "id": "wide",
                    "path": "https://w.wallhaven.cc/full/wi/wallhaven-wide.jpg",
                    "thumbs": {"original": "https://th.wallhaven.cc/orig/wi/wide.jpg"},
                    "dimension_x": 3840,
                    "dimension_y": 2160,
                },
                {
                    "id": "tall",
                    "path": "https://w.wallhaven.cc/full/ta/wallhaven-tall.png",
                    "thumbs": {"original": "https://th.wallhaven.cc/orig/ta/tall.jpg"},
                    "dimension_x": 1440,
                    "dimension_y": 3200,
                },
                {
                    "id": "elsewhere",
                    "path": "https://example.com/elsewhere.jpg",
                    "thumbs": {"original": "https://th.wallhaven.cc/orig/el/elsewhere.jpg"},
                },
            ],
            "meta": {"current_page": 1, "last_page": 4},
        });
        let page = read_page(&answer, (1264, 2780));
        let ids: Vec<&str> = page.found.iter().map(|found| found.id.as_str()).collect();
        assert_eq!(ids, ["tall", "wide"]);
        assert!(page.more);
        assert_eq!(
            file_in(Path::new("/w"), &page.found[0]),
            Path::new("/w/wallhaven-tall.png")
        );
    }

    #[test]
    #[ignore = "asks wallhaven.cc"]
    fn searches_wallhaven() {
        let page = search("", 1, (1264, 2780)).unwrap();
        for found in &page.found {
            println!(
                "{} {}x{} {}",
                found.id, found.width, found.height, found.thumb
            );
        }
        assert!(!page.found.is_empty());
        let thumb = thumbnail(&page.found[0]).unwrap();
        println!("{}", thumb.display());
    }
}
