//! The phone's language. Denial's shell and the native apps take it from
//! Denial's settings document, `localization.locale`; the apps Denial
//! starts get it as `LANG`, from Denial's application environment; the
//! system keeps it through systemd's localed, for the next session.

use zbus::Connection;

/// The languages in the order the Language row offers them, each named in
/// itself there.
pub struct Language {
    /// The word Denial keeps it as.
    pub denial: &'static str,
    pub locale: &'static str,
}

pub const LANGUAGES: [Language; 2] = [
    Language {
        denial: "english",
        locale: "en_US.UTF-8",
    },
    Language {
        denial: "simplifiedChinese",
        locale: "zh_CN.UTF-8",
    },
];

/// Sets the system's language. Denial's polkit agent asks for a password
/// where the phone's rules do not allow the change outright.
pub async fn set_system(locale: &str) -> Result<(), String> {
    let bus = Connection::system()
        .await
        .map_err(|error| error.to_string())?;
    bus.call_method(
        Some("org.freedesktop.locale1"),
        "/org/freedesktop/locale1",
        Some("org.freedesktop.locale1"),
        "SetLocale",
        &(vec![format!("LANG={locale}")], true),
    )
    .await
    .map(drop)
    .map_err(|error| error.to_string())
}
