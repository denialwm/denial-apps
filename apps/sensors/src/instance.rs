//! Sensors' own name on the session bus, `org.denialwm.Sensors`. The running
//! Sensors owns it; a second `sensors` raises its window instead of opening
//! another.

use async_channel::Sender;
use zbus::proxy;

pub const NAME: &str = "org.denialwm.Sensors";
pub const PATH: &str = "/org/denialwm/Sensors";

/// `org.denialwm.Sensors1` on Sensors' bus name; it passes on the
/// activation token to raise the window with.
pub struct Opener(pub Sender<String>);

#[zbus::interface(name = "org.denialwm.Sensors1")]
impl Opener {
    async fn open(&self, activation_token: String) {
        // Closed only once the app is quitting.
        let _ = self.0.send(activation_token).await;
    }
}

#[proxy(
    interface = "org.denialwm.Sensors1",
    default_service = "org.denialwm.Sensors",
    default_path = "/org/denialwm/Sensors"
)]
pub trait Instance {
    /// Raises the window with `activation_token` (an xdg-activation token;
    /// empty when there is none).
    fn open(&self, activation_token: &str) -> zbus::Result<()>;
}
