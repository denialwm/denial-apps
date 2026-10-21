//! Messages' own name on the session bus, `org.denialwm.Messages`. The
//! running Messages owns it; a second `messages <peer>`, or the notifier,
//! asks it to open the conversation instead of opening another window.

use zbus::proxy;

pub const NAME: &str = "org.denialwm.Messages";
pub const PATH: &str = "/org/denialwm/Messages";

#[proxy(
    interface = "org.denialwm.Messages1",
    default_service = "org.denialwm.Messages",
    default_path = "/org/denialwm/Messages"
)]
pub trait App {
    /// Shows the conversation with `peer`, or the window as it is when `peer`
    /// is empty, and raises the window with `activation_token` (an
    /// xdg-activation token; empty when there is none).
    fn open(&self, peer: &str, activation_token: &str) -> zbus::Result<()>;
}
