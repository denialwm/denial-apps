//! The kernel side of a camera: the media graph of qcom-camss, the sensor's
//! controls and the raw frames of its video node, through the V4L2 and
//! media controller ioctls. Nothing between the sensor and memory processes
//! the frames: they arrive as the sensor's packed 10-bit Bayer samples
//! (RDI), and everything after that is ours (`isp`, `photo`).

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use crate::sensor::Sensor;

const fn iowr(kind: u8, nr: u8, size: usize) -> libc::c_ulong {
    (3 << 30) | ((size as libc::c_ulong) << 16) | ((kind as libc::c_ulong) << 8) | nr as libc::c_ulong
}

const fn iow(kind: u8, nr: u8, size: usize) -> libc::c_ulong {
    (1 << 30) | ((size as libc::c_ulong) << 16) | ((kind as libc::c_ulong) << 8) | nr as libc::c_ulong
}

#[repr(C)]
struct EntityDesc {
    id: u32,
    name: [u8; 32],
    kind: u32,
    revision: u32,
    flags: u32,
    group_id: u32,
    pads: u16,
    links: u16,
    reserved: [u32; 4],
    major: u32,
    minor: u32,
    raw: [u8; 176],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PadDesc {
    entity: u32,
    index: u16,
    flags: u32,
    reserved: [u32; 2],
}

#[repr(C)]
struct LinkDesc {
    source: PadDesc,
    sink: PadDesc,
    flags: u32,
    reserved: [u32; 2],
}

#[repr(C)]
#[derive(Default)]
struct MbusFormat {
    width: u32,
    height: u32,
    code: u32,
    field: u32,
    colorspace: u32,
    ycbcr_enc: u16,
    quantization: u16,
    xfer_func: u16,
    flags: u16,
    reserved: [u16; 10],
}

#[repr(C)]
#[derive(Default)]
struct SubdevFormat {
    which: u32,
    pad: u32,
    format: MbusFormat,
    stream: u32,
    reserved: [u32; 7],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct PlaneFormat {
    sizeimage: u32,
    bytesperline: u32,
    reserved: [u16; 6],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct PixFormatMplane {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    colorspace: u32,
    planes: [PlaneFormat; 8],
    num_planes: u8,
    flags: u8,
    ycbcr_enc: u8,
    quantization: u8,
    xfer_func: u8,
    reserved: [u8; 7],
}

#[repr(C)]
struct Format {
    kind: u32,
    _pad: u32,
    pix: PixFormatMplane,
    // The kernel's union is 200 bytes and 8-byte aligned.
    _rest: [u8; 200 - std::mem::size_of::<PixFormatMplane>()],
    _align: [u64; 0],
}

#[repr(C)]
#[derive(Default)]
struct RequestBuffers {
    count: u32,
    kind: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Plane {
    bytesused: u32,
    length: u32,
    mem_offset: u64,
    data_offset: u32,
    reserved: [u32; 11],
}

#[repr(C)]
struct Buffer {
    index: u32,
    kind: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    timestamp: libc::timeval,
    timecode: [u32; 4],
    sequence: u32,
    memory: u32,
    planes: *mut Plane,
    length: u32,
    reserved2: u32,
    request_fd: i32,
}

#[repr(C)]
struct Control {
    id: u32,
    value: i32,
}

const MEDIA_IOC_ENUM_ENTITIES: libc::c_ulong = iowr(b'|', 0x01, size_of::<EntityDesc>());
const MEDIA_IOC_SETUP_LINK: libc::c_ulong = iowr(b'|', 0x03, size_of::<LinkDesc>());
const VIDIOC_SUBDEV_S_FMT: libc::c_ulong = iowr(b'V', 5, size_of::<SubdevFormat>());
const VIDIOC_S_FMT: libc::c_ulong = iowr(b'V', 5, size_of::<Format>());
const VIDIOC_REQBUFS: libc::c_ulong = iowr(b'V', 8, size_of::<RequestBuffers>());
const VIDIOC_QUERYBUF: libc::c_ulong = iowr(b'V', 9, size_of::<Buffer>());
const VIDIOC_QBUF: libc::c_ulong = iowr(b'V', 15, size_of::<Buffer>());
const VIDIOC_DQBUF: libc::c_ulong = iowr(b'V', 17, size_of::<Buffer>());
const VIDIOC_STREAMON: libc::c_ulong = iow(b'V', 18, size_of::<i32>());
const VIDIOC_STREAMOFF: libc::c_ulong = iow(b'V', 19, size_of::<i32>());
const VIDIOC_S_CTRL: libc::c_ulong = iowr(b'V', 28, size_of::<Control>());

const MEDIA_ENT_ID_FLAG_NEXT: u32 = 1 << 31;
const MEDIA_LNK_FL_ENABLED: u32 = 1;
const V4L2_SUBDEV_FORMAT_ACTIVE: u32 = 1;
const V4L2_FIELD_NONE: u32 = 1;
const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
const V4L2_MEMORY_MMAP: u32 = 1;

pub const CID_EXPOSURE: u32 = 0x0098_0911;
pub const CID_ANALOGUE_GAIN: u32 = 0x009e_0903;
pub const CID_FOCUS_ABSOLUTE: u32 = 0x009a_090a;

/// The camera subsystem's routes: every sensor goes through CSID 0 and the
/// first raw dump interface of VFE 0, so one camera streams at a time.
const CSID: &str = "msm_csid0";
const RDI: &str = "msm_vfe0_rdi0";
const VIDEO: &str = "msm_vfe0_video0";
/// CSIPHYs the SoC has; a sensor sits behind one of them.
const CSIPHYS: u32 = 8;
const CSIDS: u32 = 5;
/// Buffers queued to the video node.
const BUFFERS: u32 = 4;

fn ioctl<T>(file: &File, request: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    loop {
        // SAFETY: `arg` is the request's argument struct, laid out as the
        // kernel's, and lives across the call.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), request, arg as *mut T) };
        if result >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

/// An entity of the media graph, with the device node it has, if any.
struct Entity {
    id: u32,
    name: String,
    node: Option<PathBuf>,
}

/// The `/dev` node of character device `major:minor`.
fn node_of(major: u32, minor: u32) -> Option<PathBuf> {
    if major == 0 && minor == 0 {
        return None;
    }
    let link = PathBuf::from(format!("/dev/char/{major}:{minor}"));
    if let Ok(path) = std::fs::canonicalize(&link) {
        return Some(path);
    }
    // No /dev/char links: look for the node itself.
    let wanted = libc::makedev(major, minor);
    std::fs::read_dir("/dev").ok()?.flatten().find_map(|entry| {
        use std::os::unix::fs::MetadataExt;
        let meta = entry.metadata().ok()?;
        (meta.file_type().is_char_device() && meta.rdev() == wanted).then(|| entry.path())
    })
}

struct Graph {
    media: File,
    entities: Vec<Entity>,
}

impl Graph {
    fn open() -> io::Result<Self> {
        let media = open(Path::new("/dev/media0"))?;
        let mut entities = Vec::new();
        let mut id = 0;
        loop {
            // SAFETY: all-zero is a valid EntityDesc.
            let mut desc: EntityDesc = unsafe { std::mem::zeroed() };
            desc.id = id | MEDIA_ENT_ID_FLAG_NEXT;
            if ioctl(&media, MEDIA_IOC_ENUM_ENTITIES, &mut desc).is_err() {
                break;
            }
            let name = CStr::from_bytes_until_nul(&desc.name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            entities.push(Entity {
                id: desc.id,
                name,
                node: node_of(desc.major, desc.minor),
            });
            id = desc.id;
        }
        Ok(Self { media, entities })
    }

    fn find(&self, matches: impl Fn(&str) -> bool) -> io::Result<&Entity> {
        self.entities
            .iter()
            .find(|entity| matches(&entity.name))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "media entity missing"))
    }

    fn named(&self, name: &str) -> io::Result<&Entity> {
        self.find(|entity| entity == name)
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("no {name} in the media graph")))
    }

    fn link(&self, source: &str, source_pad: u16, sink: &str, sink_pad: u16, on: bool) -> io::Result<()> {
        let (Ok(source), Ok(sink)) = (self.named(source), self.named(sink)) else {
            // A route this SoC does not have is already off.
            return if on {
                Err(io::Error::new(io::ErrorKind::NotFound, "link endpoint missing"))
            } else {
                Ok(())
            };
        };
        let mut desc = LinkDesc {
            source: PadDesc {
                entity: source.id,
                index: source_pad,
                ..Default::default()
            },
            sink: PadDesc {
                entity: sink.id,
                index: sink_pad,
                ..Default::default()
            },
            flags: if on { MEDIA_LNK_FL_ENABLED } else { 0 },
            reserved: [0; 2],
        };
        match ioctl(&self.media, MEDIA_IOC_SETUP_LINK, &mut desc) {
            // Not a link of this graph: nothing to turn off.
            Err(error) if !on && error.raw_os_error() == Some(libc::EINVAL) => Ok(()),
            result => result.map_err(|error| {
                io::Error::new(error.kind(), format!("linking {} to {}: {error}", source.name, sink.name))
            }),
        }
    }

    fn node(&self, name: &str) -> io::Result<File> {
        let entity = self.named(name)?;
        let path = entity
            .node
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{name} has no device node")))?;
        open(path)
    }
}

/// The names of the media graph's entities.
pub fn entity_names() -> io::Result<Vec<String>> {
    Ok(Graph::open()?.entities.into_iter().map(|entity| entity.name).collect())
}

fn set_pad_format(subdev: &File, pad: u32, sensor: &Sensor) -> io::Result<()> {
    let mut format = SubdevFormat {
        which: V4L2_SUBDEV_FORMAT_ACTIVE,
        pad,
        format: MbusFormat {
            width: sensor.width,
            height: sensor.height,
            code: sensor.bayer.mbus_code(),
            field: V4L2_FIELD_NONE,
            ..Default::default()
        },
        ..Default::default()
    };
    ioctl(subdev, VIDIOC_SUBDEV_S_FMT, &mut format)
}

/// One mmap'd capture buffer.
struct Mapping {
    data: *mut u8,
    length: usize,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: mapped by `Device::open` with this length.
        unsafe { libc::munmap(self.data.cast(), self.length) };
    }
}

/// A frame the driver handed over; queue it back with `Device::release`.
pub struct Frame<'a> {
    pub data: &'a [u8],
    pub stride: usize,
}

/// One sensor, routed to memory and streaming.
pub struct Device {
    pub sensor: &'static Sensor,
    sensor_dev: File,
    lens: Option<File>,
    video: File,
    buffers: Vec<Mapping>,
    stride: usize,
    /// A frame is out with the caller.
    held: Option<u32>,
}

// The mappings are only touched from the thread that owns the device.
unsafe impl Send for Device {}

impl Device {
    /// Routes `sensor` through CSID 0 to VFE 0's raw interface, sets every
    /// pad to its mode and starts streaming.
    pub fn open(sensor: &'static Sensor) -> io::Result<Self> {
        let graph = Graph::open()?;
        let sensor_entity = graph.find(|name| name.starts_with(sensor.entity))?;
        let sensor_name = sensor_entity.name.clone();
        let csiphy = format!("msm_csiphy{}", sensor.csiphy);
        for index in 0..CSIPHYS {
            graph.link(&format!("msm_csiphy{index}"), 1, CSID, 0, false)?;
        }
        for index in 1..CSIDS {
            graph.link(&format!("msm_csid{index}"), 1, RDI, 0, false)?;
        }
        graph.link(&csiphy, 1, CSID, 0, true)?;
        graph.link(CSID, 1, RDI, 0, true)?;

        let sensor_dev = graph.node(&sensor_name)?;
        set_pad_format(&sensor_dev, 0, sensor)?;
        let phy = graph.node(&csiphy)?;
        set_pad_format(&phy, 0, sensor)?;
        set_pad_format(&phy, 1, sensor)?;
        let csid = graph.node(CSID)?;
        set_pad_format(&csid, 0, sensor)?;
        set_pad_format(&csid, 1, sensor)?;
        let rdi = graph.node(RDI)?;
        set_pad_format(&rdi, 0, sensor)?;
        set_pad_format(&rdi, 1, sensor)?;
        // The focus motor powers down a second after its node closes, so the
        // device holds it open while it streams.
        let lens = match &sensor.lens {
            Some(lens) => Some(graph.node(&graph.find(|name| name.starts_with(lens.entity))?.name.clone())?),
            None => None,
        };

        let video = graph.node(VIDEO)?;
        let mut format = Format {
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            _pad: 0,
            pix: PixFormatMplane {
                width: sensor.width,
                height: sensor.height,
                pixelformat: sensor.bayer.fourcc(),
                field: V4L2_FIELD_NONE,
                num_planes: 1,
                ..Default::default()
            },
            _rest: [0; 200 - size_of::<PixFormatMplane>()],
            _align: [],
        };
        ioctl(&video, VIDIOC_S_FMT, &mut format)?;
        let plane = format.pix.planes[0];
        let stride = plane.bytesperline as usize;
        if stride < sensor.width as usize * 5 / 4 {
            return Err(io::Error::other("video node refused the raw format"));
        }

        let mut request = RequestBuffers {
            count: BUFFERS,
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            ..Default::default()
        };
        ioctl(&video, VIDIOC_REQBUFS, &mut request)?;
        let mut device = Self {
            sensor,
            sensor_dev,
            lens,
            video,
            buffers: Vec::new(),
            stride,
            held: None,
        };
        for index in 0..request.count {
            let mut planes = [Plane::default(); 1];
            let mut buffer = device.buffer(index, &mut planes);
            ioctl(&device.video, VIDIOC_QUERYBUF, &mut buffer)?;
            let length = planes[0].length as usize;
            // SAFETY: maps the driver's buffer at the offset it gave.
            let data = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    length,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    device.video.as_raw_fd(),
                    planes[0].mem_offset as libc::off_t,
                )
            };
            if data == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            device.buffers.push(Mapping {
                data: data.cast(),
                length,
            });
            device.queue(index)?;
        }
        let mut kind = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE as i32;
        ioctl(&device.video, VIDIOC_STREAMON, &mut kind)?;
        Ok(device)
    }

    fn buffer(&self, index: u32, planes: &mut [Plane; 1]) -> Buffer {
        Buffer {
            index,
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            bytesused: 0,
            flags: 0,
            field: 0,
            timestamp: libc::timeval { tv_sec: 0, tv_usec: 0 },
            timecode: [0; 4],
            sequence: 0,
            memory: V4L2_MEMORY_MMAP,
            planes: planes.as_mut_ptr(),
            length: 1,
            reserved2: 0,
            request_fd: 0,
        }
    }

    fn queue(&self, index: u32) -> io::Result<()> {
        let mut planes = [Plane::default(); 1];
        let mut buffer = self.buffer(index, &mut planes);
        ioctl(&self.video, VIDIOC_QBUF, &mut buffer)
    }

    /// Waits up to a second for the next frame.
    pub fn next(&mut self) -> io::Result<Frame<'_>> {
        if let Some(index) = self.held.take() {
            self.queue(index)?;
        }
        let mut poll = libc::pollfd {
            fd: self.video.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one pollfd for the video node.
        let ready = unsafe { libc::poll(&mut poll, 1, 1000) };
        if ready == 0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no frame from the sensor"));
        }
        if ready < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut planes = [Plane::default(); 1];
        let mut buffer = self.buffer(0, &mut planes);
        ioctl(&self.video, VIDIOC_DQBUF, &mut buffer)?;
        let index = buffer.index;
        self.held = Some(index);
        let mapping = &self.buffers[index as usize];
        let size = self.stride * self.sensor.height as usize;
        // SAFETY: the driver filled this buffer and gave it back; it is not
        // queued again until the frame is dropped by the next `next`.
        let data = unsafe { std::slice::from_raw_parts(mapping.data, size.min(mapping.length)) };
        Ok(Frame {
            data,
            stride: self.stride,
        })
    }

    /// Bytes per row of a frame.
    pub fn stride(&self) -> usize {
        self.stride
    }

    fn control(file: &File, id: u32, value: i32) -> io::Result<()> {
        let mut control = Control { id, value };
        ioctl(file, VIDIOC_S_CTRL, &mut control)
    }

    /// Sets the exposure in lines and the analogue gain code; the sensor
    /// takes them `Sensor::DELAY` frames later.
    pub fn expose(&self, lines: u32, gain_code: u32) -> io::Result<()> {
        Self::control(&self.sensor_dev, CID_EXPOSURE, lines as i32)?;
        Self::control(&self.sensor_dev, CID_ANALOGUE_GAIN, gain_code as i32)
    }

    /// Moves the lens; returns once the motor has taken the position.
    pub fn focus(&self, position: u32) -> io::Result<()> {
        match &self.lens {
            Some(lens) => Self::control(lens, CID_FOCUS_ABSOLUTE, position as i32),
            None => Ok(()),
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let mut kind = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE as i32;
        let _ = ioctl(&self.video, VIDIOC_STREAMOFF, &mut kind);
        self.buffers.clear();
        let mut request = RequestBuffers {
            count: 0,
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            ..Default::default()
        };
        let _ = ioctl(&self.video, VIDIOC_REQBUFS, &mut request);
    }
}

const _: () = assert!(size_of::<EntityDesc>() == 256);
const _: () = assert!(size_of::<LinkDesc>() == 52);
const _: () = assert!(size_of::<SubdevFormat>() == 88);
const _: () = assert!(size_of::<Format>() == 208);
const _: () = assert!(size_of::<Buffer>() == 88);
const _: () = assert!(size_of::<Plane>() == 64);
