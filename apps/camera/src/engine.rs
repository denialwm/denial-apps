//! The camera thread. braun (Phonewave's camera service) finds the cameras,
//! streams them and develops the photos; this thread picks the camera the
//! window faces, passes on what the window asks, turns braun's previews into
//! images and names the photos. The window talks to it through `Command`s
//! and hears back through `Event`s.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;

use braun::client::{self, Info, Session};
use braun::proto::{Reply, Request};
use gpui::RenderImage;
use image::{Frame, RgbaImage};

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

    fn name(self) -> &'static str {
        match self {
            Facing::Back => "back",
            Facing::Front => "front",
        }
    }
}

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
    /// The camera facing this way is open: its frame upright is `aspect`
    /// times as tall as it is wide, and it can focus if it has a `lens`.
    Opened { facing: Facing, aspect: f32, lens: bool },
    Preview(Arc<RenderImage>),
    /// The burst is in; the photo is being developed.
    Captured,
    Saved(PathBuf),
    /// A photo failed; the camera goes on.
    PhotoFailed(String),
    /// The camera failed, or braun did.
    Failed(String),
}

/// Commands to the camera thread, which wake it.
#[derive(Clone)]
pub struct Commands {
    sender: async_channel::Sender<Command>,
    wake: Arc<OwnedFd>,
}

impl Commands {
    pub fn try_send(&self, command: Command) -> Result<(), async_channel::TrySendError<Command>> {
        self.sender.try_send(command)?;
        let one = 1u64.to_ne_bytes();
        // SAFETY: writes 8 bytes to the eventfd.
        unsafe { libc::write(self.wake.as_raw_fd(), one.as_ptr().cast(), one.len()) };
        Ok(())
    }
}

/// The camera thread's side of `Commands`.
pub struct Inbox {
    receiver: async_channel::Receiver<Command>,
    wake: Arc<OwnedFd>,
}

pub fn commands() -> (Commands, Inbox) {
    let (sender, receiver) = async_channel::bounded(8);
    // SAFETY: a new eventfd this process owns.
    let wake = Arc::new(unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)) });
    (
        Commands {
            sender,
            wake: wake.clone(),
        },
        Inbox { receiver, wake },
    )
}

/// A photo braun is taking.
struct Shot {
    path: PathBuf,
    /// Where braun writes it, until it is whole.
    partial: PathBuf,
    /// Its burst is in: it no longer needs the camera.
    captured: bool,
}

struct Engine {
    events: async_channel::Sender<Event>,
    session: Option<Session>,
    cameras: Vec<Info>,
    facing: Facing,
    zoom: f32,
    shown: bool,
    /// The camera asked for and not failed since.
    open: Option<String>,
    /// Something failed: nothing is tried again until the window asks for
    /// something, so that two apps never take the camera back and forth.
    stalled: bool,
    shots: HashMap<u32, Shot>,
    next_shot: u32,
}

/// Runs the camera until the window goes away.
pub fn run(inbox: Inbox, events: async_channel::Sender<Event>) {
    let mut engine = Engine {
        events,
        session: None,
        cameras: Vec::new(),
        facing: Facing::Back,
        zoom: 1.0,
        shown: true,
        open: None,
        stalled: false,
        shots: HashMap::new(),
        next_shot: 1,
    };
    loop {
        engine.catch_up();
        let mut fds = vec![libc::pollfd {
            fd: inbox.wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        if let Some(session) = &engine.session {
            fds.push(libc::pollfd {
                fd: session.fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        // SAFETY: `fds` holds valid pollfds for the call.
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        let mut count = [0u8; 8];
        // SAFETY: reads the eventfd's count into 8 bytes.
        unsafe { libc::read(inbox.wake.as_raw_fd(), count.as_mut_ptr().cast(), count.len()) };
        loop {
            match inbox.receiver.try_recv() {
                Ok(command) => engine.command(command),
                Err(async_channel::TryRecvError::Empty) => break,
                Err(async_channel::TryRecvError::Closed) => return,
            }
        }
        engine.replies();
        if engine.events.is_closed() {
            return;
        }
    }
}

impl Engine {
    /// Connects to braun and opens the camera when they should be and are
    /// not, unless stalled.
    fn catch_up(&mut self) {
        if self.stalled {
            return;
        }
        if self.session.is_none() {
            match Session::connect().and_then(|session| Ok((session, client::cameras()?))) {
                Ok((session, cameras)) => {
                    self.cameras = cameras;
                    let _ = session.send(&Request::Zoom(self.zoom), &[]);
                    self.session = Some(session);
                }
                Err(error) => return self.failed(format!("camera service: {error}")),
            }
        }
        if self.shown && self.open.is_none() {
            self.open_facing();
        }
    }

    /// The camera facing `facing`: its main one, else the first that is not
    /// an auxiliary camera.
    fn camera(&self, facing: Facing) -> Option<&Info> {
        let facing_this = |camera: &&Info| camera.facing == facing.name() && camera.role != "auxiliary";
        self.cameras
            .iter()
            .filter(facing_this)
            .find(|camera| camera.role == "main")
            .or_else(|| self.cameras.iter().find(facing_this))
    }

    fn open_facing(&mut self) {
        let Some(id) = self.camera(self.facing).map(|camera| camera.id.clone()) else {
            return self.failed(format!("no {} camera", self.facing.name()));
        };
        if self.send(&Request::Open(id.clone()), &[]) {
            self.open = Some(id);
        }
    }

    /// Sends `request`; false if braun is gone.
    fn send(&mut self, request: &Request, fds: &[std::os::fd::BorrowedFd<'_>]) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        if session.send(request, fds).is_ok() {
            return true;
        }
        self.lost("the camera service stopped".into());
        false
    }

    fn command(&mut self, command: Command) {
        self.stalled = false;
        match command {
            Command::Switch(facing) => {
                self.facing = facing;
                if self.shown && self.session.is_some() {
                    self.open_facing();
                }
            }
            Command::Hide(hidden) => {
                self.shown = !hidden;
                if hidden && self.open.take().is_some() {
                    self.send(&Request::Close, &[]);
                }
            }
            Command::Zoom(zoom) => {
                self.zoom = zoom;
                self.send(&Request::Zoom(zoom), &[]);
            }
            Command::Focus(x, y) => {
                self.send(&Request::Focus(x, y), &[]);
            }
            Command::Shoot => {
                if let Err(error) = self.shoot() {
                    let _ = self.events.send_blocking(Event::PhotoFailed(format!("saving the photo: {error}")));
                }
            }
        }
    }

    /// Opens the photo's files and asks braun for it. While
    /// `~/Pictures/Camera/raw` exists, each photo also keeps the raw frame
    /// the preview showed when the shutter was pressed, with how it was
    /// exposed and rendered, beside the name of the photo: `<name>.raw`
    /// (packed RAW10 as the sensor sends it) and `<name>.txt`.
    fn shoot(&mut self) -> std::io::Result<()> {
        let path = photo_path().ok_or_else(|| std::io::Error::other("no home folder"))?;
        let folder = path.parent().expect("a photo's folder").to_path_buf();
        std::fs::create_dir_all(&folder)?;
        let partial = path.with_extension("jpg.partial");
        let jpeg = std::fs::File::create(&partial)?;
        let mut files = vec![jpeg];
        let raw = folder.join("raw");
        if raw.is_dir() {
            let stem = path.file_stem().unwrap_or_default();
            files.push(std::fs::File::create(raw.join(stem).with_extension("raw"))?);
            files.push(std::fs::File::create(raw.join(stem).with_extension("txt"))?);
        }
        let shot = self.next_shot;
        self.next_shot += 1;
        let fds: Vec<_> = files.iter().map(|file| file.as_fd()).collect();
        if !self.send(&Request::Shoot(shot), &fds) {
            let _ = std::fs::remove_file(&partial);
            return Err(std::io::Error::other("the camera service stopped"));
        }
        self.shots.insert(
            shot,
            Shot {
                path,
                partial,
                captured: false,
            },
        );
        Ok(())
    }

    fn replies(&mut self) {
        loop {
            let Some(session) = &mut self.session else {
                return;
            };
            let reply = match session.next(false) {
                Ok(Some(reply)) => reply,
                Ok(None) => return,
                Err(_) => return self.lost("the camera service stopped".into()),
            };
            match reply {
                Reply::Opened { width, height, lens, .. } => {
                    let _ = self.events.send_blocking(Event::Opened {
                        facing: self.facing,
                        aspect: height as f32 / width.max(1) as f32,
                        lens,
                    });
                }
                Reply::Frame { slot, width, height, .. } => {
                    let pixels = session.frame(slot, width, height).map(<[u8]>::to_vec);
                    self.send(&Request::Release(slot), &[]);
                    let Some(image) = pixels.and_then(|pixels| RgbaImage::from_raw(width, height, pixels)) else {
                        continue;
                    };
                    // The window takes one preview at a time; one it has no
                    // time for is dropped.
                    let _ = self.events.try_send(Event::Preview(Arc::new(RenderImage::new([Frame::new(image)]))));
                }
                Reply::Captured(shot) => {
                    if let Some(shot) = self.shots.get_mut(&shot) {
                        shot.captured = true;
                    }
                    let _ = self.events.send_blocking(Event::Captured);
                }
                Reply::Saved(shot) => {
                    let Some(shot) = self.shots.remove(&shot) else {
                        continue;
                    };
                    let event = match std::fs::rename(&shot.partial, &shot.path) {
                        Ok(()) => Event::Saved(shot.path),
                        Err(error) => Event::PhotoFailed(format!("saving the photo: {error}")),
                    };
                    let _ = self.events.send_blocking(event);
                }
                Reply::Failed(0, error) => {
                    // The bursts not yet in went with the camera.
                    self.fail_shots(|shot| !shot.captured, &error);
                    self.open = None;
                    self.failed(error);
                }
                Reply::Failed(shot, error) => {
                    if let Some(shot) = self.shots.remove(&shot) {
                        let _ = std::fs::remove_file(&shot.partial);
                        let _ = self.events.send_blocking(Event::PhotoFailed(error));
                    }
                }
            }
        }
    }

    fn fail_shots(&mut self, which: impl Fn(&Shot) -> bool, error: &str) {
        let failed: Vec<u32> = self.shots.iter().filter(|(_, shot)| which(shot)).map(|(&id, _)| id).collect();
        for id in failed {
            if let Some(shot) = self.shots.remove(&id) {
                let _ = std::fs::remove_file(&shot.partial);
                let _ = self.events.send_blocking(Event::PhotoFailed(format!("saving the photo: {error}")));
            }
        }
    }

    /// braun went away: every photo with it.
    fn lost(&mut self, error: String) {
        self.session = None;
        self.open = None;
        self.fail_shots(|_| true, &error);
        self.failed(error);
    }

    fn failed(&mut self, error: String) {
        let _ = self.events.send_blocking(Event::Failed(error));
        self.stalled = true;
    }
}

/// `~/Pictures/Camera/IMG_<date>_<time>.jpg`, in local time.
fn photo_path() -> Option<PathBuf> {
    let folder = crate::library::folder()?;
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
    let mut path = folder.join(format!("{name}.jpg"));
    let mut index = 1;
    while path.exists() || path.with_extension("jpg.partial").exists() {
        path = folder.join(format!("{name}_{index}.jpg"));
        index += 1;
    }
    Some(path)
}
