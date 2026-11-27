//! Desktop notifications on the session bus, which Denial serves, and the
//! few session facts `resume` needs besides: whether the network is up, and
//! how to open Settings.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::proxy::MethodFlags;
use zbus::zvariant::Value as Variant;

use crate::util::{Error, Result};

/// One desktop notification as the user sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    pub title: String,
    pub body: String,
    /// Action keys and labels, in presentation order. The `default` key is
    /// the notification body itself.
    pub actions: Vec<(&'static str, &'static str)>,
    /// Stays visible after one of its actions was invoked.
    pub resident: bool,
    /// Never expires; the user or its sender closes it.
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Action {
        id: u32,
        key: String,
        /// Lets the launched window take focus, when the server gave one.
        token: Option<String>,
    },
    Closed {
        id: u32,
    },
}

impl Event {
    pub fn id(&self) -> u32 {
        match self {
            Event::Action { id, .. } | Event::Closed { id } => *id,
        }
    }
}

pub trait Notifier {
    /// Shows `content`, replacing notification `replaces` while it is open.
    fn show(&mut self, content: &Content, replaces: u32) -> Result<u32>;
    fn close(&mut self, id: u32) -> Result<()>;
    /// The next event, waiting at most `timeout` for one.
    fn next(&mut self, timeout: Duration) -> Option<Event>;
}

const INTERFACE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

fn bus(error: zbus::Error) -> Error {
    Error::new(error.to_string())
}

/// org.freedesktop.Notifications on the session bus.
pub struct DesktopNotifications {
    proxy: Proxy<'static>,
    events: Receiver<Event>,
}

impl DesktopNotifications {
    /// deniald claims the service from its event loop, which starts just
    /// after it launches `resume`. Wait for that owner: activating the
    /// service early could start another notification daemon in Denial's
    /// place.
    pub fn connect(wait: Duration) -> Result<Self> {
        let connection = Connection::session().map_err(bus)?;
        let daemon = zbus::blocking::fdo::DBusProxy::new(&connection).map_err(bus)?;
        let deadline = Instant::now() + wait;
        let name = zbus::names::BusName::try_from(INTERFACE)
            .map_err(|error| Error::new(error.to_string()))?;
        while !daemon
            .name_has_owner(name.clone())
            .map_err(|error| Error::new(error.to_string()))?
        {
            if Instant::now() >= deadline {
                return Err(Error::new(format!(
                    "no notification server owns {INTERFACE}"
                )));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let rule = zbus::MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(INTERFACE)
            .map_err(bus)?
            .path(PATH)
            .map_err(bus)?
            .build();
        let signals = MessageIterator::for_match_rule(rule, &connection, None).map_err(bus)?;
        let (sender, events) = channel();
        std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                // The server announces an action's token just before the
                // action itself.
                let mut tokens: HashMap<u32, String> = HashMap::new();
                for message in signals {
                    let Ok(message) = message else { continue };
                    let header = message.header();
                    let Some(member) = header.member().map(|member| member.to_string()) else {
                        continue;
                    };
                    let body = message.body();
                    let event = match member.as_str() {
                        "ActivationToken" => {
                            if let Ok((id, token)) = body.deserialize::<(u32, String)>() {
                                tokens.insert(id, token);
                            }
                            continue;
                        }
                        "ActionInvoked" => match body.deserialize::<(u32, String)>() {
                            Ok((id, key)) => Event::Action {
                                id,
                                key,
                                token: tokens.remove(&id),
                            },
                            Err(_) => continue,
                        },
                        "NotificationClosed" => match body.deserialize::<(u32, u32)>() {
                            Ok((id, _)) => {
                                tokens.remove(&id);
                                Event::Closed { id }
                            }
                            Err(_) => continue,
                        },
                        _ => continue,
                    };
                    if sender.send(event).is_err() {
                        return;
                    }
                }
            })?;
        let proxy = Proxy::new_owned(connection, INTERFACE, PATH, INTERFACE).map_err(bus)?;
        let notifications = Self { proxy, events };
        notifications
            .proxy
            .call_with_flags::<_, _, Vec<String>>(
                "GetCapabilities",
                MethodFlags::NoAutoStart.into(),
                &(),
            )
            .map_err(bus)?;
        Ok(notifications)
    }
}

impl Notifier for DesktopNotifications {
    fn show(&mut self, content: &Content, replaces: u32) -> Result<u32> {
        let actions: Vec<&str> = content
            .actions
            .iter()
            .flat_map(|(key, label)| [*key, *label])
            .collect();
        let mut hints: HashMap<&str, Variant> = HashMap::new();
        hints.insert("desktop-entry", Variant::from("org.denialwm.Settings"));
        hints.insert("urgency", Variant::U8(1));
        if content.resident {
            hints.insert("resident", Variant::Bool(true));
        }
        let id: Option<u32> = self
            .proxy
            .call_with_flags(
                "Notify",
                MethodFlags::NoAutoStart.into(),
                &(
                    "Plugins",
                    replaces,
                    "preferences-desktop",
                    content.title.as_str(),
                    content.body.as_str(),
                    actions,
                    hints,
                    if content.persistent { 0i32 } else { -1i32 },
                ),
            )
            .map_err(bus)?;
        id.ok_or_else(|| Error::new("The notification server gave no notification id"))
    }

    fn close(&mut self, id: u32) -> Result<()> {
        self.proxy
            .call_with_flags::<_, _, ()>(
                "CloseNotification",
                MethodFlags::NoAutoStart.into(),
                &(id,),
            )
            .map_err(bus)?;
        Ok(())
    }

    fn next(&mut self, timeout: Duration) -> Option<Event> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                std::thread::sleep(timeout);
                None
            }
        }
    }
}

/// NetworkManager's global connectivity: Some(true) when connected,
/// Some(false) when it knows the session is offline, None when nobody can
/// tell.
pub fn network_connected() -> Option<bool> {
    let connection = Connection::system().ok()?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .ok()?;
    let state: u32 = proxy.get_property("State").ok()?;
    // NM_STATE_CONNECTED_GLOBAL is 70; 0 means NetworkManager cannot tell.
    if state == 0 { None } else { Some(state == 70) }
}

/// The command line of an installed desktop entry, or None when it is not
/// installed. Field codes are dropped: no files or URLs are passed.
pub fn desktop_entry_command(
    id: &str,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Option<Vec<String>> {
    let home = environment("HOME");
    let data_home =
        environment("XDG_DATA_HOME").or_else(|| home.map(|home| format!("{home}/.local/share")));
    let data_dirs =
        environment("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let folders = data_home.into_iter().chain(
        data_dirs
            .split(':')
            .filter(|folder| !folder.is_empty())
            .map(str::to_owned),
    );
    for folder in folders {
        let file = PathBuf::from(folder).join("applications").join(id);
        let Ok(contents) = std::fs::read_to_string(&file) else {
            if file.exists() {
                return None;
            }
            continue;
        };
        let mut in_entry = false;
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_entry = trimmed == "[Desktop Entry]";
            } else if in_entry && let Some(exec) = trimmed.strip_prefix("Exec=") {
                let command = split_exec(exec);
                return (!command.is_empty()).then_some(command);
            }
        }
        // The first entry with this ID is the one launchers use.
        return None;
    }
    None
}

fn split_exec(value: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let characters: Vec<char> = value.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == '\\' && quoted && index + 1 < characters.len() {
            index += 1;
            current.push(characters[index]);
        } else if character == '"' {
            quoted = !quoted;
            started = true;
        } else if character == ' ' && !quoted {
            if started {
                arguments.push(std::mem::take(&mut current));
            }
            current.clear();
            started = false;
        } else {
            current.push(character);
            started = true;
        }
        index += 1;
    }
    if started {
        arguments.push(current);
    }
    arguments
        .into_iter()
        .filter(|argument| {
            let bytes = argument.as_bytes();
            !(bytes.len() == 2 && bytes[0] == b'%' && bytes[1].is_ascii_alphabetic())
        })
        .map(|argument| argument.replace("%%", "%"))
        .collect()
}

/// Starts a detached process that may take focus with `token`.
pub fn launch_detached(command: &[String], token: Option<&str>) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let Some((program, arguments)) = command.split_first() else {
        return Ok(());
    };
    let mut process = std::process::Command::new(program);
    process
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    if let Some(token) = token {
        process
            .env("XDG_ACTIVATION_TOKEN", token)
            .env("DESKTOP_STARTUP_ID", token);
    }
    let mut child = process.spawn()?;
    // Reaped when it ends, so it leaves no zombie behind while this runs.
    std::thread::spawn(move || child.wait());
    Ok(())
}
