//! Following the contacts folder with inotify, so edits show without a
//! restart. The folder may not exist yet: its parent is watched too, for the
//! folder being made, removed or renamed. Its subfolders, the collections,
//! are watched as they come and go.

use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_io::{Async, Timer};
use futures_lite::future;

/// A sync writes many cards in a row; changes this close together make one.
const SETTLE: Duration = Duration::from_millis(150);
/// Inside the folder and its collections: cards written in place or renamed
/// over, and removed; in the folder, collections made too.
const FOLDER_EVENTS: u32 = libc::IN_CLOSE_WRITE
    | libc::IN_MOVED_TO
    | libc::IN_MOVED_FROM
    | libc::IN_DELETE
    | libc::IN_CREATE;
/// In its parent: the folder itself coming and going.
const PARENT_EVENTS: u32 =
    libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_MOVED_FROM | libc::IN_DELETE | libc::IN_ONLYDIR;
/// The fixed part of a `struct inotify_event`, before its name.
const EVENT_HEADER: usize = 16;

pub struct Watch {
    inotify: Async<OwnedFd>,
    /// The folder's own name in its parent.
    name: PathBuf,
    folder: PathBuf,
    parent: i32,
    /// The watch inside the folder, while it exists.
    inside: Option<i32>,
    /// The watch inside each collection, with the collection's name.
    collections: Vec<(i32, OsString)>,
}

impl Watch {
    pub fn new(folder: &Path) -> io::Result<Self> {
        let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the contacts folder has no parent",
            ));
        };
        // SAFETY: inotify_init1 takes flags only and returns a new descriptor.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh descriptor nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // The parent first: a folder made in between is then still seen.
        let parent = add_watch(&fd, parent, PARENT_EVENTS)?;
        let mut watch = Self {
            inotify: Async::new(fd)?,
            name: name.into(),
            folder: folder.to_owned(),
            parent,
            inside: None,
            collections: Vec::new(),
        };
        watch.enter();
        Ok(watch)
    }

    /// Watches inside the folder and each collection in it, as far as they
    /// exist.
    fn enter(&mut self) {
        self.inside = add_watch(self.inotify.get_ref(), &self.folder, FOLDER_EVENTS).ok();
        if self.inside.is_none() {
            return;
        }
        for entry in std::fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .flatten()
        {
            if entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !crate::hidden(&entry.file_name())
            {
                self.add_collection(entry.file_name());
            }
        }
    }

    fn add_collection(&mut self, name: OsString) {
        let path = self.folder.join(&name);
        if let Ok(wd) = add_watch(self.inotify.get_ref(), &path, FOLDER_EVENTS) {
            self.collections.retain(|(other, _)| *other != wd);
            self.collections.push((wd, name));
        }
    }

    fn remove_watch(&self, wd: i32) {
        // A folder renamed away keeps its watch; one removed has lost it
        // already, and this fails harmlessly.
        // SAFETY: inotify_rm_watch only takes the two numbers.
        unsafe { libc::inotify_rm_watch(self.inotify.as_raw_fd(), wd) };
    }

    /// Waits until cards in the folder changed, or the folder came or went,
    /// and the changes have settled.
    pub async fn changed(&mut self) -> io::Result<()> {
        while !self.read().await? {}
        let mut settled = Instant::now() + SETTLE;
        loop {
            let read = future::or(async { Some(self.read().await) }, async {
                Timer::at(settled).await;
                None
            });
            match read.await {
                None => return Ok(()),
                Some(changed) => {
                    if changed? {
                        settled = Instant::now() + SETTLE;
                    }
                }
            }
        }
    }

    /// Reads the events queued now; whether any of them is about the cards.
    async fn read(&mut self) -> io::Result<bool> {
        let mut buffer = [0u8; 4096];
        let length = self
            .inotify
            .read_with(|fd| {
                // SAFETY: read writes at most buffer.len() bytes into buffer.
                let n =
                    unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
                usize::try_from(n).map_err(|_| io::Error::last_os_error())
            })
            .await?;
        let mut changed = false;
        let mut events = &buffer[..length];
        while events.len() >= EVENT_HEADER {
            let field = |at: usize| u32::from_ne_bytes(events[at..at + 4].try_into().unwrap());
            let (wd, mask, size) = (field(0) as i32, field(4), field(12) as usize);
            let name = events
                .get(EVENT_HEADER..EVENT_HEADER + size)
                .unwrap_or_default();
            let name = Path::new(std::ffi::OsStr::from_bytes(
                name.split(|&b| b == 0).next().unwrap_or_default(),
            ));
            events = events.get(EVENT_HEADER + size..).unwrap_or_default();
            changed |= self.take(wd, mask, name);
        }
        Ok(changed)
    }

    /// Follows one event; whether it is about the cards.
    fn take(&mut self, wd: i32, mask: u32, name: &Path) -> bool {
        if mask & libc::IN_Q_OVERFLOW != 0 {
            return true;
        }
        let arrived = mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0;
        if wd == self.parent && name == self.name {
            if arrived {
                self.enter();
            } else {
                for wd in self.inside.take().into_iter().chain(
                    std::mem::take(&mut self.collections)
                        .into_iter()
                        .map(|(wd, _)| wd),
                ) {
                    self.remove_watch(wd);
                }
            }
            return true;
        }
        if mask & libc::IN_IGNORED != 0 {
            if Some(wd) == self.inside {
                self.inside = None;
            }
            self.collections.retain(|(other, _)| *other != wd);
            return false;
        }
        if Some(wd) == self.inside && mask & libc::IN_ISDIR != 0 {
            if crate::hidden(name.as_os_str()) {
                return false;
            }
            if arrived {
                self.add_collection(name.as_os_str().to_owned());
            } else if let Some(at) = self.collections.iter().position(|(_, n)| n == name) {
                let (wd, _) = self.collections.swap_remove(at);
                self.remove_watch(wd);
            }
            return true;
        }
        let watched = Some(wd) == self.inside || self.collections.iter().any(|(w, _)| *w == wd);
        watched && mask & libc::IN_CREATE == 0 && crate::is_card(name)
    }
}

fn add_watch(fd: &OwnedFd, path: &Path, mask: u32) -> io::Result<i32> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: path is a NUL-terminated string that outlives the call.
    let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) };
    if wd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(wd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_the_folder_made_and_cards_changed() {
        let root =
            std::env::temp_dir().join(format!("denial-contacts-watch-{}", std::process::id()));
        let folder = root.join("contacts");
        std::fs::create_dir_all(&root).unwrap();
        let mut watch = Watch::new(&folder).unwrap();
        future::block_on(async {
            std::fs::create_dir(&folder).unwrap();
            watch.changed().await.unwrap();
            std::fs::write(folder.join("notes.txt"), "not a card").unwrap();
            assert!(!watch.read_now());
            std::fs::write(folder.join("a.vcf"), "BEGIN:VCARD\nFN:A\nEND:VCARD\n").unwrap();
            watch.changed().await.unwrap();
            std::fs::remove_file(folder.join("a.vcf")).unwrap();
            watch.changed().await.unwrap();
            // A collection made, then a card in it.
            std::fs::create_dir(folder.join("default")).unwrap();
            watch.changed().await.unwrap();
            std::fs::write(
                folder.join("default/b.vcf"),
                "BEGIN:VCARD\nFN:B\nEND:VCARD\n",
            )
            .unwrap();
            watch.changed().await.unwrap();
        });
        std::fs::remove_dir_all(&root).unwrap();
    }

    impl Watch {
        /// Whether a change about the cards is queued, without waiting.
        fn read_now(&mut self) -> bool {
            future::block_on(future::or(async { self.read().await.unwrap() }, async {
                Timer::after(Duration::from_millis(50)).await;
                false
            }))
        }
    }
}
