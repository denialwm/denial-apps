//! Following Denial's settings.json with inotify, so a change in Denial's
//! settings reaches the app without a restart. Denial replaces the file
//! atomically, writing beside it and renaming over it, which a watch on the
//! file itself would lose; the folder is watched for the name instead.

use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;

use async_io::Async;

/// The file renamed into place, or written in place.
const EVENTS: u32 = libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE;
/// The fixed part of a `struct inotify_event`, before its name.
const EVENT_HEADER: usize = 16;

pub struct SettingsWatch {
    inotify: Async<OwnedFd>,
    name: OsString,
}

impl SettingsWatch {
    /// Watches settings.json; fails when its folder does not exist.
    pub fn new() -> io::Result<Self> {
        let path = crate::settings_path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        };
        // SAFETY: inotify_init1 takes flags only and returns a new descriptor.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh descriptor nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let folder = CString::new(folder.as_os_str().as_bytes())?;
        // SAFETY: folder is a NUL-terminated string that outlives the call.
        if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), folder.as_ptr(), EVENTS) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            inotify: Async::new(fd)?,
            name: name.to_owned(),
        })
    }

    /// Waits until settings.json was replaced or written. Changes queued
    /// together count once.
    pub async fn changed(&mut self) -> io::Result<()> {
        loop {
            let mut buffer = [0u8; 4096];
            let length = self
                .inotify
                .read_with(|fd| {
                    // SAFETY: read writes at most buffer.len() bytes into buffer.
                    let n = unsafe {
                        libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len())
                    };
                    usize::try_from(n).map_err(|_| io::Error::last_os_error())
                })
                .await?;
            let mut events = &buffer[..length];
            let mut changed = false;
            while events.len() >= EVENT_HEADER {
                let size = u32::from_ne_bytes(events[12..16].try_into().unwrap()) as usize;
                let name = events
                    .get(EVENT_HEADER..EVENT_HEADER + size)
                    .unwrap_or_default();
                let name = name.split(|&b| b == 0).next().unwrap_or_default();
                changed |= name == self.name.as_bytes();
                events = events.get(EVENT_HEADER + size..).unwrap_or_default();
            }
            if changed {
                return Ok(());
            }
        }
    }
}
