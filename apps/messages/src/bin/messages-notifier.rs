//! Posts a notification for every SMS dmail receives, as the default SMS app
//! does on Android; dmail itself never notifies. One notification per
//! conversation, showing its newest message under the contact's name. Tapping
//! it opens Messages on that conversation, in the running Messages if there is
//! one; its "Mark as read" action marks it read in dmail. A conversation read
//! or deleted anywhere takes its notification away. Started at login by
//! messages-notifier.service.

use std::collections::HashMap;
use std::process::{Command, ExitCode};

use denial_contacts::Contacts;
use futures_lite::{StreamExt, future};
use messages::app::AppProxy;
use messages::dmail::Messages1Proxy;
use zbus::Connection;
use zbus::proxy;
use zbus::zvariant::Value;

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn activation_token(&self, id: u32, activation_token: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

enum Event {
    Received(String),
    /// The conversation with this peer was read or deleted.
    Done(String),
    Invoked(u32, String),
    Token(u32, String),
    Closed(u32),
}

fn main() -> ExitCode {
    match future::block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("messages-notifier: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> zbus::Result<()> {
    let dmail = Messages1Proxy::new(&Connection::system().await?).await?;
    let session = Connection::session().await?;
    let notifications = NotificationsProxy::new(&session).await?;
    let app = AppProxy::new(&session).await?;
    let mut received = dmail.receive_message_received().await?;
    let mut read = dmail.receive_conversation_read().await?;
    let mut deleted = dmail.receive_conversation_deleted().await?;
    let mut invoked = notifications.receive_action_invoked().await?;
    let mut tokens = notifications.receive_activation_token().await?;
    let mut closed = notifications.receive_notification_closed().await?;
    // Peer of each notification this program posted.
    let mut posted: HashMap<u32, String> = HashMap::new();
    let mut token: Option<(u32, String)> = None;
    loop {
        let event = future::or(
            future::or(
                future::or(
                    async {
                        let signal = received.next().await?;
                        Some(Event::Received(signal.args().ok()?.peer().clone()))
                    },
                    future::or(
                        async {
                            let signal = read.next().await?;
                            Some(Event::Done(signal.args().ok()?.peer().clone()))
                        },
                        async {
                            let signal = deleted.next().await?;
                            Some(Event::Done(signal.args().ok()?.peer().clone()))
                        },
                    ),
                ),
                async {
                    let signal = invoked.next().await?;
                    let args = signal.args().ok()?;
                    Some(Event::Invoked(*args.id(), args.action_key().clone()))
                },
            ),
            future::or(
                async {
                    let signal = tokens.next().await?;
                    let args = signal.args().ok()?;
                    Some(Event::Token(*args.id(), args.activation_token().clone()))
                },
                async {
                    let signal = closed.next().await?;
                    Some(Event::Closed(*signal.args().ok()?.id()))
                },
            ),
        );
        match event.await {
            Some(Event::Received(peer)) => {
                let replaces = posted
                    .iter()
                    .find_map(|(&id, p)| (*p == peer).then_some(id))
                    .unwrap_or(0);
                match post(&dmail, &notifications, &peer, replaces).await {
                    Ok(id) => {
                        posted.remove(&replaces);
                        posted.insert(id, peer);
                    }
                    Err(error) => eprintln!("messages-notifier: notifying: {error}"),
                }
            }
            Some(Event::Done(peer)) => {
                let Some(id) = posted
                    .iter()
                    .find_map(|(&id, p)| (*p == peer).then_some(id))
                else {
                    continue;
                };
                posted.remove(&id);
                if let Err(error) = notifications.close_notification(id).await {
                    eprintln!("messages-notifier: closing a notification: {error}");
                }
            }
            // Denial sends the token just before the action.
            Some(Event::Token(id, value)) => token = Some((id, value)),
            // Denial closes the notification after an action.
            Some(Event::Invoked(id, action)) => {
                let Some(peer) = posted.remove(&id) else {
                    continue;
                };
                match action.as_str() {
                    "default" => {
                        let token = token
                            .take()
                            .filter(|(for_id, _)| *for_id == id)
                            .map(|(_, t)| t);
                        open_messages(&app, &peer, token).await;
                    }
                    "read" => {
                        if let Err(error) = dmail.mark_read(&peer).await {
                            eprintln!("messages-notifier: marking read: {error}");
                        }
                    }
                    _ => {}
                }
            }
            Some(Event::Closed(id)) => {
                posted.remove(&id);
            }
            None => return Ok(()),
        }
    }
}

/// Shows the conversation's newest message, in place of its last
/// notification, under the contact's name when the number has one.
async fn post(
    dmail: &Messages1Proxy<'_>,
    notifications: &NotificationsProxy<'_>,
    peer: &str,
    replaces: u32,
) -> zbus::Result<u32> {
    let newest = dmail.list_messages(peer, 0, 1).await?;
    let text = newest.first().map(|m| m.2.as_str()).unwrap_or_default();
    // Read afresh each time: messages are rare and the folder can change.
    let contacts = Contacts::folder().map(|folder| Contacts::load(&folder));
    let name = contacts
        .as_ref()
        .and_then(|contacts| Some(contacts.all()[contacts.find(peer)?].name.as_str()));
    let hints = HashMap::from([
        ("desktop-entry", Value::from(messages::app::NAME)),
        ("category", Value::from("im.received")),
    ]);
    notifications
        .notify(
            "Messages",
            replaces,
            "internet-mail",
            name.unwrap_or(peer),
            text,
            &["default", "Open", "read", "Mark as read"],
            hints,
            -1,
        )
        .await
}

/// Shows the conversation with `peer` in the running Messages, or starts
/// Messages next to this program on it. Either way the activation token
/// raises the window.
async fn open_messages(app: &AppProxy<'_>, peer: &str, token: Option<String>) {
    let token = token.unwrap_or_default();
    match app.open(peer, &token).await {
        Ok(()) => return,
        Err(zbus::Error::MethodError(name, ..))
            if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" => {}
        Err(error) => return eprintln!("messages-notifier: opening Messages: {error}"),
    }
    let app = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("messages"),
        Err(error) => return eprintln!("messages-notifier: {error}"),
    };
    let mut command = Command::new(&app);
    command.arg(peer);
    if !token.is_empty() {
        command.env("XDG_ACTIVATION_TOKEN", token);
    }
    match command.spawn() {
        // Reaped on its own thread so it never lingers as a zombie.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(error) => eprintln!("messages-notifier: {}: {error}", app.display()),
    }
}
