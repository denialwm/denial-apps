//! dmail's `org.dmail.Voice1`, the modem's calls, and callaudio's
//! `org.callaudio.CallAudio1`, which mutes the microphone during one. See
//! Phonewave's services/modem/dmail/DESIGN.md and services/audio/callaudio.

use zbus::proxy;

/// Id, number (empty when withheld), direction (`in` or `out`), state, and
/// the Unix time it connected (0 until then).
pub type CallRow = (u8, String, String, String, i64);
/// A call of the log: id, number (empty when withheld), direction (`in` or
/// `out`), outcome (answered, missed, declined, unanswered), and the Unix
/// times it started, connected (0 if it never did) and ended.
pub type LoggedCall = (u64, String, String, String, i64, i64, i64);

#[proxy(
    interface = "org.dmail.Voice1",
    default_service = "org.dmail",
    default_path = "/org/dmail"
)]
pub trait Voice1 {
    /// States: dialing, alerting, incoming, waiting, active, held, ending.
    /// Ended calls leave the list.
    #[zbus(property)]
    fn calls(&self) -> zbus::Result<Vec<CallRow>>;

    /// Returns the new call's id. A number is digits, +, * and #.
    fn dial(&self, number: &str) -> zbus::Result<u8>;

    fn answer(&self, id: u8) -> zbus::Result<()>;

    /// Ends a call, or turns down an incoming one.
    fn hangup(&self, id: u8) -> zbus::Result<()>;

    /// Returns once the digits have sounded.
    fn send_dtmf(&self, id: u8, digits: &str) -> zbus::Result<()>;

    /// Up to `limit` logged calls with ids below `before` (0 for the newest),
    /// newest first.
    fn list_call_log(&self, before: u64, limit: u32) -> zbus::Result<Vec<LoggedCall>>;

    /// CallLogDeleted follows.
    fn delete_logged_call(&self, id: u64) -> zbus::Result<()>;

    /// A call that ended went into the log.
    #[zbus(signal)]
    fn call_logged(&self, id: u64) -> zbus::Result<()>;

    #[zbus(signal)]
    fn call_log_deleted(&self, id: u64) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.callaudio.CallAudio1",
    default_service = "org.callaudio",
    default_path = "/org/callaudio"
)]
pub trait CallAudio1 {
    /// The other side hears nothing; each call starts unmuted.
    #[zbus(property)]
    fn muted(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_muted(&self, muted: bool) -> zbus::Result<()>;
}
