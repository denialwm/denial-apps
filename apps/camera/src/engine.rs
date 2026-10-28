//! The camera thread: streams one sensor, renders its preview, runs auto
//! exposure, white balance and the focus sweep, and takes the burst a photo
//! is developed from. The window talks to it through `Command`s and hears
//! back through `Event`s.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::RenderImage;
use image::{Frame, RgbaImage};

use crate::device::Device;
use crate::isp::{self, Exposure, Preview, Rendering, Tone};
use crate::photo::{self, Group};
use crate::sensor::{self, Facing, Lens, Sensor};

/// Frames the sensor takes to apply new exposure controls, plus one.
const SETTLE: usize = 3;
/// Auto exposure looks again after this many frames, once the last change
/// has landed.
const AE_EVERY: usize = 3;
/// The burst, relative to the exposure auto exposure chose: a group longer
/// for the shadows, a shorter one, and a much shorter one for the lights.
const BRACKET: [(f32, usize); 3] = [(2.0, 8), (0.25, 4), (1.0 / 16.0, 4)];
/// The focus sweep: coarse over the whole travel, then fine around the
/// best coarse position. The steps are for a travel of `TRAVEL` positions
/// and scale with the lens's.
const COARSE: u32 = 80;
const FINE: u32 = 16;
const TRAVEL: u32 = 1023;
/// Waiting this long for the camera to come back after an error.
const RETRY: Duration = Duration::from_secs(2);

pub enum Command {
    Switch(Facing),
    /// The window is hidden (true) or shown again: the camera closes while
    /// no one can see it.
    Hide(bool),
    Shoot,
    /// Focus on this point of the preview, 0 to 1 across and down.
    Focus(f32, f32),
    /// Crop the middle `1 / zoom` of the frame, in the preview and the
    /// photos.
    Zoom(f32),
}

pub enum Event {
    Opened(Facing),
    Preview(Arc<RenderImage>),
    /// The burst is in; the photo is being developed.
    Captured,
    Saved(PathBuf),
    Failed(String),
}

struct Sweep {
    x: f32,
    y: f32,
    positions: Vec<u32>,
    next: usize,
    /// Frames to skip before the next measurement.
    wait: usize,
    scores: Vec<(u32, f32)>,
    fine: bool,
    travel: RangeInclusive<u32>,
    coarse_step: u32,
    fine_step: u32,
}

impl Sweep {
    fn coarse(x: f32, y: f32, lens: &Lens) -> Self {
        let span = lens.travel.end() - lens.travel.start();
        let coarse_step = COARSE * span / TRAVEL;
        Self {
            x,
            y,
            positions: lens.travel.clone().step_by(coarse_step as usize).collect(),
            next: 0,
            wait: 0,
            scores: Vec::new(),
            fine: false,
            travel: lens.travel.clone(),
            coarse_step,
            fine_step: FINE * span / TRAVEL,
        }
    }
}

/// Why streaming stopped.
enum Stop {
    Switch(Facing),
    Hide,
    Closed,
}

/// Runs the camera until the window goes away.
pub fn run(commands: async_channel::Receiver<Command>, events: async_channel::Sender<Event>) {
    let tone = Tone::new();
    let mut facing = Facing::Back;
    let mut zoom = 1.0;
    loop {
        let sensor = sensor::of(facing);
        match stream(sensor, facing, &mut zoom, &commands, &events, &tone) {
            Ok(Stop::Switch(next)) => facing = next,
            Ok(Stop::Hide) => {
                if !wait_shown(&commands, &mut facing, &mut zoom) {
                    return;
                }
            }
            Ok(Stop::Closed) => return,
            Err(error) => {
                if events
                    .send_blocking(Event::Failed(format!("{}: {error}", sensor.name)))
                    .is_err()
                {
                    return;
                }
                // Try again, unless asked for the other camera meanwhile.
                match commands.recv_blocking() {
                    Ok(Command::Switch(next)) => facing = next,
                    Ok(Command::Zoom(next)) => zoom = next,
                    Ok(Command::Hide(true)) => {
                        if !wait_shown(&commands, &mut facing, &mut zoom) {
                            return;
                        }
                    }
                    Ok(_) => std::thread::sleep(RETRY),
                    Err(_) => return,
                }
            }
        }
    }
}

/// Keeps the camera closed until the window is shown again, following which
/// camera is asked for meanwhile; false once the window is gone.
fn wait_shown(commands: &async_channel::Receiver<Command>, facing: &mut Facing, zoom: &mut f32) -> bool {
    loop {
        match commands.recv_blocking() {
            Ok(Command::Hide(false)) => return true,
            Ok(Command::Switch(next)) => *facing = next,
            Ok(Command::Zoom(next)) => *zoom = next,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

/// Streams `sensor` until another camera is asked for, the window is hidden
/// or it is gone.
fn stream(
    sensor: &'static Sensor,
    facing: Facing,
    zoom: &mut f32,
    commands: &async_channel::Receiver<Command>,
    events: &async_channel::Sender<Event>,
    tone: &Tone,
) -> std::io::Result<Stop> {
    let mut device = Device::open(sensor)?;
    let mut exposure = Exposure::of(20_000.0, sensor);
    device.expose(exposure.lines, sensor.gain.code(exposure.gain))?;
    let mut rendering = Rendering::neutral(sensor.colour);
    let mut sweep = sensor.lens.as_ref().map(|lens| Sweep::coarse(0.5, 0.5, lens));
    let mut focus = sensor.lens.as_ref().map_or(0, |lens| lens.start);
    device.focus(focus)?;
    if events.send_blocking(Event::Opened(facing)).is_err() {
        return Ok(Stop::Closed);
    }
    let mut since_exposure = 0;
    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Switch(next) if next != facing => return Ok(Stop::Switch(next)),
                Command::Switch(_) | Command::Hide(false) => {}
                Command::Hide(true) => return Ok(Stop::Hide),
                Command::Zoom(next) => *zoom = next,
                Command::Focus(x, y) => {
                    if let Some(lens) = &sensor.lens {
                        sweep = Some(Sweep::coarse(x, y, lens));
                    }
                }
                Command::Shoot => {
                    let path = photo_path();
                    keep_raw(&mut device, &path, exposure, &rendering)?;
                    let groups = burst(&mut device, exposure)?;
                    if events.send_blocking(Event::Captured).is_err() {
                        return Ok(Stop::Closed);
                    }
                    let stride = device.stride();
                    let events = events.clone();
                    let rendering = rendering;
                    let base = exposure.on_sensor(sensor);
                    let zoom = *zoom;
                    std::thread::spawn(move || {
                        let event = match photo::develop(&groups, sensor, stride, base, exposure.digital, zoom, &rendering, &path) {
                            Ok(()) => Event::Saved(path),
                            Err(error) => Event::Failed(format!("saving the photo: {error}")),
                        };
                        let _ = events.send_blocking(event);
                    });
                    // Back to the preview's exposure.
                    device.expose(exposure.lines, sensor.gain.code(exposure.gain))?;
                    since_exposure = 0;
                }
            }
        }
        if commands.is_closed() {
            return Ok(Stop::Closed);
        }

        let frame = device.next()?;
        let (preview, stats) = isp::preview(frame.data, frame.stride, sensor, &rendering, exposure.digital, *zoom, tone);
        since_exposure += 1;
        if since_exposure >= AE_EVERY {
            if let Some(next) = isp::meter(&stats, exposure, sensor) {
                exposure = next;
                device.expose(exposure.lines, sensor.gain.code(exposure.gain))?;
                since_exposure = 0;
            }
        }
        isp::balance(&stats, &mut rendering, sensor.colour, exposure.digital);
        if let Some(active) = &mut sweep {
            if step_sweep(active, &preview, &device, &mut focus)? {
                sweep = None;
            }
        }
        let image = Arc::new(RenderImage::new([Frame::new(
            RgbaImage::from_raw(preview.width as u32, preview.height as u32, preview.bgra)
                .expect("preview size"),
        )]));
        match events.try_send(Event::Preview(image)) {
            Ok(()) | Err(async_channel::TrySendError::Full(_)) => {}
            Err(async_channel::TrySendError::Closed(_)) => return Ok(Stop::Closed),
        }
    }
}

/// Measures the frame for the sweep and moves the lens on; true once the
/// lens is at the sharpest position found.
fn step_sweep(sweep: &mut Sweep, preview: &Preview, device: &Device, focus: &mut u32) -> std::io::Result<bool> {
    if sweep.wait > 0 {
        sweep.wait -= 1;
        return Ok(false);
    }
    if sweep.next > 0 {
        let at = sweep.positions[sweep.next - 1];
        sweep.scores.push((at, isp::sharpness(preview, sweep.x, sweep.y)));
    }
    if sweep.next == sweep.positions.len() {
        let best = sweep
            .scores
            .iter()
            .fold((sweep.positions[0], f32::MIN), |a, &b| if b.1 > a.1 { b } else { a })
            .0;
        if sweep.fine {
            *focus = best;
            device.focus(best)?;
            return Ok(true);
        }
        sweep.fine = true;
        let start = best.saturating_sub(sweep.coarse_step / 2).max(*sweep.travel.start());
        let end = (best + sweep.coarse_step / 2).min(*sweep.travel.end());
        sweep.positions = (start..=end).step_by(sweep.fine_step as usize).collect();
        sweep.scores.clear();
        sweep.next = 0;
    }
    let at = sweep.positions[sweep.next];
    device.focus(at)?;
    sweep.next += 1;
    // The frame being exposed while the lens moves is soft; measure the next.
    sweep.wait = 1;
    Ok(false)
}

/// Takes the bracketed burst around `exposure`.
fn burst(device: &mut Device, exposure: Exposure) -> std::io::Result<Vec<Group>> {
    let sensor = device.sensor;
    let mut groups = Vec::new();
    for (factor, count) in BRACKET {
        let group_exposure = Exposure::of(exposure.on_sensor(sensor) * factor, sensor);
        device.expose(group_exposure.lines, sensor.gain.code(group_exposure.gain))?;
        for _ in 0..SETTLE {
            device.next()?;
        }
        let mut frames = Vec::with_capacity(count);
        for _ in 0..count {
            frames.push(device.next()?.data.to_vec());
        }
        groups.push(Group {
            exposure: group_exposure.on_sensor(sensor),
            frames,
        });
    }
    Ok(groups)
}

/// While `~/Pictures/Camera/raw` exists, each photo also keeps the raw
/// frame the preview showed when the shutter was pressed, with how it was
/// exposed and rendered, beside the name of the photo: `<name>.raw` (packed
/// RAW10 as the sensor sends it) and `<name>.txt`.
fn keep_raw(device: &mut Device, photo: &Path, exposure: Exposure, rendering: &Rendering) -> std::io::Result<()> {
    let Some(folder) = photo.parent().map(|folder| folder.join("raw")).filter(|folder| folder.is_dir()) else {
        return Ok(());
    };
    let stem = photo.file_stem().unwrap_or_default();
    let sensor = device.sensor;
    let stride = device.stride();
    let frame = device.next()?;
    std::fs::write(folder.join(stem).with_extension("raw"), frame.data)?;
    let notes = format!(
        "sensor {}\nwidth {}\nheight {}\nstride {}\nbayer {:?}\nblack {}\nlines {}\ngain {}\ndigital {}\nexposure_us {}\nwb {} {} {}\nlight {}\nct {}\nccm {}\n",
        sensor.name,
        sensor.width,
        sensor.height,
        stride,
        sensor.bayer,
        sensor.black,
        exposure.lines,
        exposure.gain,
        exposure.digital,
        exposure.lines as f32 * sensor.line_us,
        rendering.wb[0],
        rendering.wb[1],
        rendering.wb[2],
        rendering.light,
        rendering.ct,
        rendering.ccm.map(|value| value.to_string()).join(" "),
    );
    std::fs::write(folder.join(stem).with_extension("txt"), notes)
}

/// `~/Pictures/Camera/IMG_<date>_<time>.jpg`, in local time.
fn photo_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: localtime_r fills `tm` from `now`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    let name = format!(
        "IMG_{:04}{:02}{:02}_{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    let folder = home.join("Pictures/Camera");
    let mut path = folder.join(format!("{name}.jpg"));
    let mut index = 1;
    while path.exists() {
        path = folder.join(format!("{name}_{index}.jpg"));
        index += 1;
    }
    path
}
