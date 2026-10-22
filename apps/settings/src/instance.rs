//! Settings' own name on the session bus, `org.denialwm.Settings`. The
//! running Settings owns it; a second `settings <page>` asks it to show the
//! page instead of opening another window.

use async_channel::Sender;
use zbus::proxy;

pub const NAME: &str = "org.denialwm.Settings";
pub const PATH: &str = "/org/denialwm/Settings";

/// A page the running Settings is asked to show, or none for the window as
/// it is, with the activation token to raise the window with.
pub type Open = (String, String);

/// `org.denialwm.Settings1` on Settings' bus name.
pub struct Opener(pub Sender<Open>);

#[zbus::interface(name = "org.denialwm.Settings1")]
impl Opener {
    async fn open(&self, page: String, activation_token: String) {
        // Closed only once the app is quitting.
        let _ = self.0.send((page, activation_token)).await;
    }
}

#[proxy(
    interface = "org.denialwm.Settings1",
    default_service = "org.denialwm.Settings",
    default_path = "/org/denialwm/Settings"
)]
pub trait Instance {
    /// Shows the page `page` (its id, as `wifi`), or the window as it is when
    /// `page` is empty, and raises the window with `activation_token` (an
    /// xdg-activation token; empty when there is none).
    fn open(&self, page: &str, activation_token: &str) -> zbus::Result<()>;
}
