//! The photos in ~/Pictures/Camera, newest first, and their images for the
//! gallery: square thumbnails, cached in ~/.cache/denial-camera/thumbnails,
//! screen-sized photos, and the whole photo while it is zoomed, decoded on a
//! thread of their own so the window never waits for a JPEG.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, RgbImage, RgbaImage};

/// Side of a thumbnail, px: a third of the Ace 3's width.
const THUMBNAIL: u32 = 420;
/// Longest side of a photo as the viewer shows it, px.
const SCREEN: u32 = 2048;
const THUMBNAIL_QUALITY: u8 = 85;

pub enum Request {
    Thumbnail(PathBuf),
    Screen(PathBuf),
    Full(PathBuf),
}

pub enum Loaded {
    Thumbnail(PathBuf, Arc<RenderImage>),
    Screen(PathBuf, Arc<RenderImage>),
    Full(PathBuf, Arc<RenderImage>),
    Failed(PathBuf),
}

pub fn folder() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Pictures/Camera"))
}

/// The photos, newest first: their names are their date and time.
pub fn photos() -> Vec<PathBuf> {
    let Some(folder) = folder() else {
        return Vec::new();
    };
    let mut photos: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "jpg"))
                .collect()
        })
        .unwrap_or_default();
    photos.sort_by(|a, b| b.cmp(a));
    photos
}

/// Removes `photo` and its thumbnail. The raw kept beside it, if any, stays:
/// it belongs to the tuning work, not to the gallery.
pub fn delete(photo: &Path) -> std::io::Result<()> {
    std::fs::remove_file(photo)?;
    if let Some(thumbnail) = thumbnail_path(photo) {
        let _ = std::fs::remove_file(thumbnail);
    }
    Ok(())
}

fn thumbnail_path(photo: &Path) -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(cache.join("denial-camera/thumbnails").join(photo.file_name()?))
}

/// Serves `requests` until the window goes away: the zoomed photo first,
/// then screen photos, as someone is looking at them, then thumbnails in the
/// order asked. Only the latest zoomed photo asked for is decoded.
pub fn run(requests: async_channel::Receiver<Request>, loaded: async_channel::Sender<Loaded>) {
    let mut full = None;
    let mut screens = VecDeque::new();
    let mut thumbnails = VecDeque::new();
    loop {
        let wait = full.is_none() && screens.is_empty() && thumbnails.is_empty();
        let next = if wait {
            requests.recv_blocking().ok()
        } else {
            requests.try_recv().ok()
        };
        let mut take = |request| match request {
            Request::Full(path) => full = Some(path),
            Request::Screen(path) => screens.push_back(path),
            Request::Thumbnail(path) => thumbnails.push_back(path),
        };
        match next {
            Some(request) => take(request),
            None if wait => return,
            None => {}
        }
        // Take everything asked meanwhile before choosing.
        while let Ok(request) = requests.try_recv() {
            take(request);
        }
        let event = if let Some(path) = full.take() {
            match image::open(&path) {
                Ok(image) => Loaded::Full(path, render(&image.into_rgb8())),
                Err(_) => Loaded::Failed(path),
            }
        } else if let Some(path) = screens.pop_front() {
            match screen(&path) {
                Some(image) => Loaded::Screen(path, image),
                None => Loaded::Failed(path),
            }
        } else if let Some(path) = thumbnails.pop_front() {
            match thumbnail(&path) {
                Some(image) => Loaded::Thumbnail(path, image),
                None => Loaded::Failed(path),
            }
        } else {
            continue;
        };
        if loaded.send_blocking(event).is_err() {
            return;
        }
    }
}

fn screen(photo: &Path) -> Option<Arc<RenderImage>> {
    let rgb = image::open(photo).ok()?.into_rgb8();
    let (width, height) = rgb.dimensions();
    let scale = (SCREEN as f32 / width.max(height) as f32).min(1.0);
    let rgb = if scale < 1.0 {
        image::imageops::thumbnail(
            &rgb,
            (width as f32 * scale).round() as u32,
            (height as f32 * scale).round() as u32,
        )
    } else {
        rgb
    };
    Some(render(&rgb))
}

/// The middle square of `photo`, from the cache when it is as new as the
/// photo.
fn thumbnail(photo: &Path) -> Option<Arc<RenderImage>> {
    let cached = thumbnail_path(photo);
    let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
    if let Some(cached) = &cached
        && modified(cached) >= modified(photo)
        && let Ok(image) = image::open(cached)
    {
        return Some(render(&image.into_rgb8()));
    }
    let rgb = image::open(photo).ok()?.into_rgb8();
    let (width, height) = rgb.dimensions();
    let side = width.min(height);
    let square = image::imageops::crop_imm(&rgb, (width - side) / 2, (height - side) / 2, side, side).to_image();
    let small = image::imageops::thumbnail(&square, THUMBNAIL, THUMBNAIL);
    if let Some(cached) = cached {
        let saved = cached
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::File::create(&cached))
            .and_then(|file| {
                image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), THUMBNAIL_QUALITY)
                    .encode_image(&small)
                    .map_err(std::io::Error::other)
            });
        if let Err(error) = saved {
            eprintln!("camera: caching the thumbnail of {}: {error}", photo.display());
        }
    }
    Some(render(&small))
}

/// GPUI takes BGRA.
fn render(rgb: &RgbImage) -> Arc<RenderImage> {
    let (width, height) = rgb.dimensions();
    let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in rgb.pixels() {
        let [r, g, b] = pixel.0;
        bgra.extend_from_slice(&[b, g, r, 255]);
    }
    Arc::new(RenderImage::new([Frame::new(
        RgbaImage::from_raw(width, height, bgra).expect("image size"),
    )]))
}

/// When a photo was taken, from its name: year, month, day, hour, minute.
pub fn taken(photo: &Path) -> Option<[u32; 5]> {
    let stem = photo.file_stem()?.to_str()?;
    let digits = stem.strip_prefix("IMG_")?;
    let number = |range: std::ops::Range<usize>| digits.get(range)?.parse::<u32>().ok();
    Some([number(0..4)?, number(4..6)?, number(6..8)?, number(9..11)?, number(11..13)?])
}

/// What the gallery tells about a photo.
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// From the raw notes kept beside the photo, if any: the sensor, the
    /// exposure in µs, the analogue and digital gain.
    pub exposure: Option<(String, f32, f32, f32)>,
}

pub fn info(photo: &Path) -> Option<Info> {
    let (width, height) = image::image_dimensions(photo).ok()?;
    let bytes = std::fs::metadata(photo).ok()?.len();
    let exposure = photo
        .parent()
        .zip(photo.file_stem())
        .and_then(|(folder, stem)| std::fs::read_to_string(folder.join("raw").join(stem).with_extension("txt")).ok())
        .and_then(|notes| {
            let field = |name: &str| {
                notes
                    .lines()
                    .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
                    .map(str::to_owned)
            };
            let number = |name: &str| field(name)?.parse::<f32>().ok();
            Some((field("sensor")?, number("exposure_us")?, number("gain")?, number("digital").unwrap_or(1.0)))
        });
    Some(Info {
        width,
        height,
        bytes,
        exposure,
    })
}
