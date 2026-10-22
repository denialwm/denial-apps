//! The values the user gave the settings, kept on the phone in
//! `$XDG_STATE_HOME/denial/settings/values.json`, one entry per changed
//! setting by its key. A setting never changed keeps the catalog's start.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::catalog::{Item, Kind};

fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|dir| dir.join("denial/settings/values.json"))
}

pub struct Values {
    path: Option<PathBuf>,
    kept: Map<String, Value>,
}

impl Values {
    pub fn load() -> Self {
        let path = path();
        let kept = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, kept }
    }

    /// Whether a toggle is on.
    pub fn on(&self, item: &Item) -> bool {
        let Kind::Toggle(start) = item.kind else {
            return false;
        };
        self.kept
            .get(item.key)
            .and_then(Value::as_bool)
            .unwrap_or(start)
    }

    /// A slider's level, 0 to 1.
    pub fn level(&self, item: &Item) -> f32 {
        let Kind::Slider(start) = item.kind else {
            return 0.0;
        };
        self.kept
            .get(item.key)
            .and_then(Value::as_f64)
            .map_or(start, |level| (level as f32).clamp(0.0, 1.0))
    }

    /// The option a choice or a segmented setting is on.
    pub fn picked(&self, item: &Item) -> usize {
        let (Kind::Segmented(options, start) | Kind::Choice(options, start)) = item.kind else {
            return 0;
        };
        self.kept
            .get(item.key)
            .and_then(Value::as_u64)
            .map(|picked| picked as usize)
            .filter(|&picked| picked < options.len())
            .unwrap_or(start)
    }

    /// Sets `item` to `value` and keeps it on the phone.
    pub fn set(&mut self, item: &Item, value: impl Into<Value>) {
        self.set_unsaved(item, value);
        self.save();
    }

    /// Sets `item` to `value` for now: a slider while it is dragged.
    pub fn set_unsaved(&mut self, item: &Item, value: impl Into<Value>) {
        self.kept.insert(item.key.to_owned(), value.into());
    }

    /// Writes the values beside the file and renames over it, so a crash
    /// never leaves half a file.
    pub fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let temporary = path.with_extension("new");
        let saved = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&temporary, Value::Object(self.kept.clone()).to_string()))
            .and_then(|()| std::fs::rename(&temporary, path));
        if let Err(error) = saved {
            eprintln!("settings: saving {}: {error}", path.display());
        }
    }
}
