//! The phone's name: systemd's pretty hostname (`org.freedesktop.hostname1`
//! on the system bus). Android under Droidloom takes it as its device name;
//! Settings gives Bluetooth the same name.

use zbus::Connection;

/// The pretty hostname in machine-info(5); None when unset.
pub fn read() -> Option<String> {
    let info = std::fs::read_to_string("/etc/machine-info").ok()?;
    pretty_hostname(&info)
}

fn pretty_hostname(info: &str) -> Option<String> {
    let raw = info.lines().find_map(|line| {
        let (key, raw) = line.trim().split_once('=')?;
        (key.trim() == "PRETTY_HOSTNAME").then_some(raw.trim())
    })?;
    let name = match raw.as_bytes() {
        [b'"', .., b'"'] if raw.len() >= 2 => {
            let mut name = String::new();
            let mut escaped = false;
            for c in raw[1..raw.len() - 1].chars() {
                if escaped || c != '\\' {
                    name.push(c);
                    escaped = false;
                } else {
                    escaped = true;
                }
            }
            name
        }
        [b'\'', .., b'\''] if raw.len() >= 2 => raw[1..raw.len() - 1].to_owned(),
        _ => raw.to_owned(),
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Renames the phone. Denial's polkit agent asks for a password where the
/// phone's rules do not allow the change outright.
pub async fn set(name: &str) -> Result<(), String> {
    let bus = Connection::system()
        .await
        .map_err(|error| error.to_string())?;
    bus.call_method(
        Some("org.freedesktop.hostname1"),
        "/org/freedesktop/hostname1",
        Some("org.freedesktop.hostname1"),
        "SetPrettyHostname",
        &(name, true),
    )
    .await
    .map(drop)
    .map_err(|error| match error {
        zbus::Error::MethodError(name, _, _)
            if name.as_str() == "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
                || name.as_str().ends_with("AccessDenied") =>
        {
            "Not allowed".to_owned()
        }
        zbus::Error::MethodError(_, Some(said), _) => said,
        error => error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_and_plain_names() {
        let info = "CHASSIS=handset\nPRETTY_HOSTNAME=\"Kai's \\\"phone\\\"\"\n";
        assert_eq!(pretty_hostname(info).as_deref(), Some("Kai's \"phone\""));
        assert_eq!(
            pretty_hostname("PRETTY_HOSTNAME=Aston").as_deref(),
            Some("Aston")
        );
        assert_eq!(pretty_hostname("PRETTY_HOSTNAME=\"\""), None);
        assert_eq!(pretty_hostname("CHASSIS=handset"), None);
    }
}
