//! What Calls keeps on the phone across restarts, in
//! `$XDG_STATE_HOME/denial/calls/`: the favourite contacts in `favourites`,
//! by `People::key`, as strings each ended by a NUL, and `samples-hidden`,
//! there while the sample contacts and calls are hidden. The call log is not
//! kept: calls are mocked, and start afresh from the samples.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The file `name` in Calls' state folder.
fn path(name: &str) -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|dir| dir.join("denial/calls").join(name))
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
        eprintln!("calls: saving {}: {error}", path.display());
    }
}

/// The favourite contacts, by key.
pub struct Favourites {
    path: Option<PathBuf>,
    keys: BTreeSet<String>,
}

impl Favourites {
    /// The saved favourites, or `first` while none were ever saved.
    pub fn load(first: &[&str]) -> Self {
        let path = path("favourites");
        let saved = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());
        let keys = match saved {
            Some(saved) => saved.split_terminator('\0').map(str::to_owned).collect(),
            None => first.iter().map(|&key| key.to_owned()).collect(),
        };
        Self { path, keys }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    /// Makes `key` a favourite or not, and saves when that changed anything.
    pub fn set(&mut self, key: &str, favourite: bool) {
        let changed = if favourite {
            self.keys.insert(key.to_owned())
        } else {
            self.keys.remove(key)
        };
        if changed {
            write(self.path.as_ref(), self.keys.iter().map(String::as_str));
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
            eprintln!("calls: saving {}: {error}", path.display());
        }
    }
}
