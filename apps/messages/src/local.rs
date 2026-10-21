//! What Messages keeps on the phone itself rather than in dmail, across
//! restarts, in `$XDG_STATE_HOME/denial/messages/`: unsent text per
//! conversation in `drafts`, pinned conversations in `pins`, those marked
//! unread in `unread`, and `samples`, there while the sample conversations
//! are shown. Each file is strings ended by a NUL: peer and text
//! pairs for drafts, peers for the others. Neither holds a NUL: both travel
//! as D-Bus strings, which cannot, so a NUL that reaches a draft is dropped.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// The file `name` in Messages' state folder.
fn path(name: &str) -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|dir| dir.join("denial/messages").join(name))
}

/// The NUL-ended strings saved at `path`.
fn read(path: Option<&PathBuf>) -> Vec<String> {
    let saved = path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    saved.split_terminator('\0').map(str::to_owned).collect()
}

/// Saves `fields` at `path`, each ended by a NUL.
fn write<'a>(path: Option<&PathBuf>, fields: impl Iterator<Item = &'a str>) {
    let Some(path) = path else {
        return;
    };
    let data: String = fields.flat_map(|field| [field, "\0"]).collect();
    // Written beside and renamed over, so a crash never leaves half a file.
    let temporary = path.with_extension("new");
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, data))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = saved {
        eprintln!("messages: saving {}: {error}", path.display());
    }
}

pub struct Drafts {
    path: Option<PathBuf>,
    texts: HashMap<String, String>,
}

impl Drafts {
    pub fn load() -> Self {
        let path = path("drafts");
        let fields = read(path.as_ref());
        let texts = fields
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();
        Self { path, texts }
    }

    pub fn get(&self, peer: &str) -> Option<&str> {
        self.texts.get(peer).map(String::as_str)
    }

    /// Keeps `text` as the draft for `peer`, or drops the draft when `text` is
    /// blank, and saves when that changed anything.
    pub fn set(&mut self, peer: &str, text: &str) {
        let text = text.replace('\0', "");
        let changed = if text.trim().is_empty() {
            self.texts.remove(peer).is_some()
        } else {
            self.texts.insert(peer.to_owned(), text.clone()) != Some(text)
        };
        if changed {
            write(
                self.path.as_ref(),
                self.texts
                    .iter()
                    .flat_map(|(peer, text)| [peer.as_str(), text.as_str()]),
            );
        }
    }
}

/// Conversations, by peer, that carry a mark of the app's own.
pub struct Peers {
    path: Option<PathBuf>,
    peers: BTreeSet<String>,
}

impl Peers {
    /// The set saved under `name`.
    pub fn load(name: &str) -> Self {
        let path = path(name);
        let peers = read(path.as_ref()).into_iter().collect();
        Self { path, peers }
    }

    pub fn contains(&self, peer: &str) -> bool {
        self.peers.contains(peer)
    }

    /// Marks `peer` or takes its mark away, and saves when that changed
    /// anything.
    pub fn set(&mut self, peer: &str, marked: bool) {
        let changed = if marked {
            self.peers.insert(peer.to_owned())
        } else {
            self.peers.remove(peer)
        };
        if changed {
            write(self.path.as_ref(), self.peers.iter().map(String::as_str));
        }
    }
}

/// A setting that is on while its file exists.
pub struct Flag {
    path: Option<PathBuf>,
    on: bool,
}

impl Flag {
    pub fn load(name: &str) -> Self {
        let path = path(name);
        let on = path.as_ref().is_some_and(|path| path.exists());
        Self { path, on }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Turns the setting on or off, and saves when that changed it.
    pub fn set(&mut self, on: bool) {
        if on == self.on {
            return;
        }
        self.on = on;
        if on {
            write(self.path.as_ref(), std::iter::empty());
        } else if let Some(path) = &self.path
            && let Err(error) = std::fs::remove_file(path)
        {
            eprintln!("messages: saving {}: {error}", path.display());
        }
    }
}
