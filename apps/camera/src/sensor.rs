//! The phone's cameras: how each sensor is wired, timed and tuned. Values
//! for the Ace 3 (aston). Colour is the stock tuning of each module: the
//! QTech IMX890 (as libcamera's imx890.yaml in Phonewave has it) and the
//! Sunny S5K3P9, from the facts exports in ~/.cache/phonewave/aston.

use crate::shading;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Back,
    Front,
}

impl Facing {
    pub fn other(self) -> Self {
        match self {
            Facing::Back => Facing::Front,
            Facing::Front => Facing::Back,
        }
    }
}

/// The colour filter layout of the sensor's 2x2 cells.
#[derive(Clone, Copy, Debug)]
pub enum Bayer {
    Bggr,
    Grbg,
}

impl Bayer {
    pub fn mbus_code(self) -> u32 {
        match self {
            Bayer::Bggr => 0x3007,
            Bayer::Grbg => 0x300a,
        }
    }

    /// The packed 10-bit pixel format the video node writes.
    pub fn fourcc(self) -> u32 {
        let code = match self {
            Bayer::Bggr => *b"pBAA",
            Bayer::Grbg => *b"pgAA",
        };
        u32::from_le_bytes(code)
    }

    /// Where red, the two greens and blue sit in a cell, as (x, y).
    pub fn cell(self) -> [(usize, usize); 4] {
        match self {
            Bayer::Bggr => [(1, 1), (1, 0), (0, 1), (0, 0)],
            Bayer::Grbg => [(1, 0), (0, 0), (1, 1), (0, 1)],
        }
    }
}

/// How the sensor's analogue gain control maps to gain.
#[derive(Clone, Copy, Debug)]
pub enum Gain {
    /// Sony: gain = 16384 / (16384 - code).
    Sony,
    /// code = gain x `unit`.
    Linear { unit: f32 },
}

impl Gain {
    pub fn code(self, gain: f32) -> u32 {
        match self {
            Gain::Sony => (16384.0 - 16384.0 / gain).round().max(0.0) as u32,
            Gain::Linear { unit } => (gain * unit).round() as u32,
        }
    }

    pub fn gain(self, code: u32) -> f32 {
        match self {
            Gain::Sony => 16384.0 / (16384.0 - code as f32),
            Gain::Linear { unit } => code as f32 / unit,
        }
    }
}

/// One point of the illuminant locus: the R/G and B/G of grey under a
/// light of `ct` kelvin.
pub struct LocusPoint {
    pub ct: f32,
    pub rg: f32,
    pub bg: f32,
}

/// A colour correction matrix from a light of `ct` kelvin on.
pub struct Ccm {
    pub ct: f32,
    pub m: [f32; 9],
}

pub struct Colour {
    /// Grey under the known illuminants, warm to cool; empty when the
    /// sensor has no tuning, which leaves white balance to grey world.
    pub locus: &'static [LocusPoint],
    /// By colour temperature, warm to cool; one matrix for all lights when
    /// there is one entry.
    pub ccms: &'static [Ccm],
    /// Lens shading by colour temperature, warm to cool; empty for none.
    pub shading: &'static [Shading],
}

/// Lens shading grid nodes across and down the sensor.
pub const SHADING_COLUMNS: usize = 17;
pub const SHADING_ROWS: usize = 13;
pub const SHADING_NODES: usize = SHADING_COLUMNS * SHADING_ROWS;

/// The gains, red, green and blue, that undo the lens's fall-off under a
/// light of `ct` kelvin: row by row, corner to corner over the sensor.
pub struct Shading {
    pub ct: f32,
    pub gains: [[f32; SHADING_NODES]; 3],
}

pub struct Sensor {
    /// Shown in errors.
    pub name: &'static str,
    /// The start of the sensor's media entity name.
    pub entity: &'static str,
    /// The CSIPHY it is wired to.
    pub csiphy: u32,
    pub width: u32,
    pub height: u32,
    pub bayer: Bayer,
    /// In 10-bit codes.
    pub black: f32,
    /// A raw sample at or over this is clipped.
    pub clip: f32,
    /// Line time in microseconds: exposure lines to time.
    pub line_us: f32,
    pub min_lines: u32,
    pub max_lines: u32,
    /// Exposure lines are a multiple of this.
    pub line_step: u32,
    pub gain: Gain,
    /// The most analogue gain auto exposure uses.
    pub max_gain: f32,
    /// Clockwise rotation that puts the image upright on the phone held
    /// upright, from the sensor's `camera_sensor_rotation`.
    pub rotation: u32,
    /// The image is shown and kept mirrored, as a mirror would show it.
    pub mirror: bool,
    /// The start of the focus motor's media entity name.
    pub lens: Option<&'static str>,
    /// Lens position, 0 to 1023, before the first focus sweep ends.
    pub focus: u32,
    pub colour: &'static Colour,
}

/// The main camera: Sony IMX890 over C-PHY, 4096x3072 binned, 30 fps.
pub static BACK: Sensor = Sensor {
    name: "IMX890",
    entity: "imx890",
    csiphy: 2,
    width: 4096,
    height: 3072,
    bayer: Bayer::Bggr,
    black: 64.0,
    clip: 1000.0,
    // 15616 pixels per line at 1.69984 GHz.
    line_us: 15616.0 / 1699.84,
    min_lines: 10,
    max_lines: 3580,
    line_step: 2,
    gain: Gain::Sony,
    max_gain: 16.0,
    rotation: 90,
    mirror: false,
    lens: Some("dw9719"),
    // Sharp at about 30 cm.
    focus: 416,
    colour: &IMX890,
};

/// The front camera: Samsung S5K3P9, 2304x1728 binned, 30 fps.
pub static FRONT: Sensor = Sensor {
    name: "S5K3P9",
    entity: "s5k3p9",
    csiphy: 1,
    width: 2304,
    height: 1728,
    bayer: Bayer::Grbg,
    black: 64.0,
    clip: 1000.0,
    // 6144 pixels per line at 559.54 MHz.
    line_us: 6144.0 / 559.542857,
    min_lines: 3,
    max_lines: 3005,
    line_step: 1,
    gain: Gain::Linear { unit: 32.0 },
    max_gain: 16.0,
    // The driver reports 270, but streams the sensor unflipped where the
    // stock driver flips it both ways: upright is half a turn from 270.
    rotation: 90,
    mirror: true,
    lens: None,
    focus: 0,
    colour: &S5K3P9,
};

pub fn of(facing: Facing) -> &'static Sensor {
    match facing {
        Facing::Back => &BACK,
        Facing::Front => &FRONT,
    }
}

/// Stock AWB reference points of the QTech module (blackbody and daylight
/// illuminants; "High CCT" stores no temperature, 10000 K is assumed).
static IMX890: Colour = Colour {
    locus: &[
        LocusPoint { ct: 2300.0, rg: 1.0038, bg: 0.3092 },
        LocusPoint { ct: 2850.0, rg: 0.8169, bg: 0.3750 },
        LocusPoint { ct: 5000.0, rg: 0.5400, bg: 0.5852 },
        LocusPoint { ct: 6500.0, rg: 0.4648, bg: 0.7211 },
        LocusPoint { ct: 7500.0, rg: 0.4502, bg: 0.7518 },
        LocusPoint { ct: 10000.0, rg: 0.2455, bg: 1.1379 },
    ],
    // The bright, non-flash branch, keyed to the start of each CCT trigger.
    ccms: &[
        Ccm { ct: 2500.0, m: [1.46, -0.47, 0.01, -0.35, 1.28, 0.07, -0.14, -0.63, 1.77] },
        Ccm { ct: 2700.0, m: [1.51, -0.58, 0.07, -0.35, 1.31, 0.04, -0.02, -0.76, 1.78] },
        Ccm { ct: 3400.0, m: [1.51, -0.58, 0.07, -0.30, 1.26, 0.04, -0.02, -0.76, 1.78] },
        Ccm { ct: 3800.0, m: [1.55, -0.50, -0.05, -0.16, 1.30, -0.14, 0.02, -0.60, 1.58] },
        Ccm { ct: 4400.0, m: [1.55, -0.50, -0.05, -0.18, 1.30, -0.12, -0.01, -0.55, 1.56] },
        Ccm { ct: 4800.0, m: [1.55, -0.52, -0.03, -0.15, 1.34, -0.19, 0.01, -0.50, 1.49] },
        Ccm { ct: 5400.0, m: [1.55, -0.51, -0.04, -0.14, 1.35, -0.21, 0.02, -0.53, 1.51] },
        Ccm { ct: 6400.0, m: [1.58, -0.56, -0.02, -0.14, 1.30, -0.16, 0.02, -0.64, 1.62] },
        Ccm { ct: 6600.0, m: [1.77, -0.80, 0.03, -0.17, 1.28, -0.11, -0.03, -0.64, 1.67] },
        Ccm { ct: 7400.0, m: [1.77, -0.80, 0.03, -0.18, 1.28, -0.10, -0.05, -0.66, 1.71] },
    ],
    shading: shading::IMX890,
};

/// Stock AWB reference points of the Sunny module, as for the IMX890.
static S5K3P9: Colour = Colour {
    locus: &[
        LocusPoint { ct: 2300.0, rg: 1.0527, bg: 0.3123 },
        LocusPoint { ct: 2850.0, rg: 0.8561, bg: 0.3643 },
        LocusPoint { ct: 5000.0, rg: 0.5360, bg: 0.5778 },
        LocusPoint { ct: 6500.0, rg: 0.4787, bg: 0.6871 },
        LocusPoint { ct: 7500.0, rg: 0.4412, bg: 0.7355 },
        LocusPoint { ct: 10000.0, rg: 0.2180, bg: 1.1201 },
    ],
    // The bright branch (lux index up to 160), keyed to the start of each
    // CCT trigger; the first trigger starts at 1 K and ends at 2580 K.
    ccms: &[
        Ccm { ct: 2580.0, m: [1.726, -0.723, -0.003, -0.243, 1.084, 0.159, -0.016, -0.766, 1.782] },
        Ccm { ct: 2700.0, m: [1.726, -0.723, -0.003, -0.243, 1.084, 0.159, -0.016, -0.766, 1.782] },
        Ccm { ct: 3400.0, m: [1.639, -0.765, 0.126, -0.216, 1.354, -0.138, 0.061, -0.671, 1.610] },
        Ccm { ct: 4700.0, m: [1.508, -0.516, 0.008, -0.139, 1.285, -0.146, 0.021, -0.433, 1.412] },
    ],
    shading: shading::S5K3P9,
};

/// Where pixel (x, y) of a `width` x `height` image lands once turned
/// `rotation` degrees clockwise and mirrored if `mirror`, and the turned
/// image's size.
pub fn place(x: usize, y: usize, width: usize, height: usize, rotation: u32, mirror: bool) -> (usize, usize) {
    let (out_width, _) = turned(width, height, rotation);
    let (ox, oy) = match rotation {
        90 => (height - 1 - y, x),
        180 => (width - 1 - x, height - 1 - y),
        270 => (y, width - 1 - x),
        _ => (x, y),
    };
    if mirror { (out_width - 1 - ox, oy) } else { (ox, oy) }
}

pub fn turned(width: usize, height: usize, rotation: u32) -> (usize, usize) {
    if rotation % 180 == 90 { (height, width) } else { (width, height) }
}
