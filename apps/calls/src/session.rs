//! Whether the session is locked: logind's `LockedHint` on the user's
//! graphical session, which Denial keeps. Calls is not in that session
//! itself when Phonewave's calls-manager starts it, so it asks for the
//! user's.

use zbus::Connection;

use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub trait Manager {
    fn get_user(&self, uid: u32) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1"
)]
pub trait User {
    /// The user's graphical session: its id and object.
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
pub trait Session {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
}

/// The user's graphical session, on the system bus.
pub async fn graphical(bus: &Connection) -> zbus::Result<SessionProxy<'static>> {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let user = ManagerProxy::new(bus).await?.get_user(uid).await?;
    let user = UserProxy::builder(bus).path(user)?.build().await?;
    let (_, session) = user.display().await?;
    SessionProxy::builder(bus).path(session)?.build().await
}
