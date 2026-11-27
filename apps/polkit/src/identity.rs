//! How an account Polkit offers is shown: a name a person reads and a
//! picture. Missing or unreadable sources fall back to the account name and
//! a monogram; none of this decides anything.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Pictures larger than this are not shown.
const LARGEST_PICTURE: u64 = 4 * 1024 * 1024;

pub struct Profile {
    pub display_name: String,
    pub picture: Option<PathBuf>,
}

/// The profile of the account `name` (uid `uid`).
pub fn profile(uid: u32, name: &str) -> Profile {
    // Names become parts of paths below; never follow anything unusual.
    if !plain_account_name(name) {
        return Profile {
            display_name: name.to_owned(),
            picture: None,
        };
    }
    let current = ["USER", "LOGNAME"]
        .into_iter()
        .any(|key| std::env::var(key).is_ok_and(|value| value == name));
    let (mut display_name, mut picture) = if current {
        denial_profile()
    } else {
        (None, None)
    };
    if uid == 0 && display_name.is_none() {
        display_name = Some(denial_look_gpui::say("Administrator").to_string());
    }
    let display_name = display_name
        .or_else(|| passwd_real_name(name))
        .unwrap_or_else(|| name.to_owned());
    picture = picture
        .or_else(|| shown_picture(Path::new(&format!("/var/lib/AccountsService/icons/{name}"))));
    if current
        && let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && home.is_absolute()
    {
        picture = picture
            .or_else(|| shown_picture(&home.join(".face.icon")))
            .or_else(|| shown_picture(&home.join(".face")));
    }
    Profile {
        display_name,
        picture,
    }
}

/// `^[A-Za-z0-9._][A-Za-z0-9._-]*\$?$`, as useradd accepts.
fn plain_account_name(name: &str) -> bool {
    let name = name.strip_suffix('$').unwrap_or(name);
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric() || first == '.' || first == '_')
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Denial's own profile, shown when the user authenticates as themselves.
/// Read only; Settings is its one writer.
fn denial_profile() -> (Option<String>, Option<PathBuf>) {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        });
    let Some(root) = root else {
        return (None, None);
    };
    let folder = root.join("denial/plugins/denial_desktop/profile");
    let document = folder.join("profile.json");
    if std::fs::metadata(&document).map_or(true, |meta| meta.len() > 65_536) {
        return (None, None);
    }
    let Some(value) = std::fs::read(&document)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|value| value["schema"] == 1)
    else {
        return (None, None);
    };
    let name = value["displayName"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= 200)
        .map(str::to_owned);
    let picture = value["avatar"]
        .as_str()
        .filter(|avatar| denial_avatar_name(avatar))
        .and_then(|avatar| shown_picture(&folder.join(avatar)));
    (name, picture)
}

/// `avatar-<digits>-<digits>.png`, the names Settings gives pictures.
fn denial_avatar_name(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix("avatar-")
        .and_then(|rest| rest.strip_suffix(".png"))
    else {
        return false;
    };
    let mut parts = middle.split('-');
    let digits = |part: Option<&str>| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    };
    digits(parts.next()) && digits(parts.next()) && parts.next().is_none()
}

/// The real name in the GECOS field of `/etc/passwd`, when it says more
/// than the account name.
fn passwd_real_name(name: &str) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    let fields: Vec<&str> = passwd
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|fields| fields.len() >= 5 && fields[0] == name)?;
    let real = fields[4].split(',').next()?.trim();
    (!real.is_empty() && real != name).then(|| real.to_owned())
}

/// `path` when it is a regular file of a size worth showing.
fn shown_picture(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() > 0 && meta.len() <= LARGEST_PICTURE).then(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_that_become_paths_are_plain() {
        assert!(plain_account_name("logix"));
        assert!(plain_account_name("machine$"));
        assert!(plain_account_name("_apt"));
        assert!(!plain_account_name("-x"));
        assert!(!plain_account_name("../root"));
        assert!(!plain_account_name("a/b"));
        assert!(!plain_account_name(""));
    }

    #[test]
    fn only_settings_picture_names_are_followed() {
        assert!(denial_avatar_name("avatar-1-2.png"));
        assert!(!denial_avatar_name("avatar-1.png"));
        assert!(!denial_avatar_name("avatar-1-2-3.png"));
        assert!(!denial_avatar_name("avatar-../x-2.png"));
    }
}
