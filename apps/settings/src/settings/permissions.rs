//! Answers to Denial's keyboard-shortcut inhibition permission.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use denial_look_gpui::{Icon, island, say};
use futures_lite::{FutureExt, StreamExt};
use gpui::{Context, Div, SharedString, Task, prelude::*};
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

use super::{Dialog, Layer, Settings};

const SERVICE: &str = "org.freedesktop.impl.portal.PermissionStore";
const PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const TABLE: &str = "denial";
const ID: &str = "shortcuts-inhibit";
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Ask,
    Allow,
    Deny,
}

impl Answer {
    const ALL: [Self; 3] = [Self::Ask, Self::Allow, Self::Deny];

    fn from_store(value: Option<&String>) -> Self {
        match value.map(String::as_str) {
            Some("yes") => Self::Allow,
            Some("no") => Self::Deny,
            _ => Self::Ask,
        }
    }

    fn stored(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "yes",
            Self::Deny => "no",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Allow => "Allow",
            Self::Deny => "Deny",
        }
    }
}

#[derive(Default)]
pub(super) struct Permissions {
    answers: Option<BTreeMap<String, Answer>>,
    unavailable: bool,
    following: Option<Task<()>>,
    saving: HashSet<String>,
}

async fn lookup(bus: &Connection) -> zbus::Result<BTreeMap<String, Answer>> {
    let reply = bus
        .call_method(Some(SERVICE), PATH, Some(SERVICE), "Lookup", &(TABLE, ID))
        .await;
    match reply {
        Ok(reply) => {
            let (values, _): (HashMap<String, Vec<String>>, OwnedValue) =
                reply.body().deserialize()?;
            Ok(values
                .into_iter()
                .map(|(app, values)| (app, Answer::from_store(values.first())))
                .collect())
        }
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.portal.Error.NotFound" =>
        {
            Ok(BTreeMap::new())
        }
        Err(error) => Err(error),
    }
}

async fn with_timeout<T>(operation: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    operation
        .or(async {
            async_io::Timer::after(TIMEOUT).await;
            Err(zbus::Error::Failure("Permission store timed out".into()))
        })
        .await
}

async fn store_updates() -> zbus::Result<(Connection, MessageStream)> {
    let bus = Connection::session().await?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(SERVICE)?
        .interface(SERVICE)?
        .path(PATH)?
        .member("Changed")?
        .arg(0, TABLE)?
        .arg(1, ID)?
        .build();
    let changes = MessageStream::for_match_rule(rule, &bus, Some(32)).await?;
    Ok((bus, changes))
}

impl Settings {
    pub(super) fn follow_permissions(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("permissions") {
            self.permissions.following = None;
            return;
        }
        if self.permissions.following.is_some() {
            return;
        }
        self.permissions.following = Some(cx.spawn(async move |this, cx| {
            loop {
                if let Ok((bus, mut changes)) = with_timeout(store_updates()).await {
                    while let Ok(answers) = with_timeout(lookup(&bus)).await {
                        if this
                            .update(cx, |view, cx| {
                                view.permissions.answers = Some(answers);
                                view.permissions.unavailable = false;
                                cx.notify();
                            })
                            .is_err()
                        {
                            return;
                        }
                        if !matches!(changes.next().await, Some(Ok(_))) {
                            break;
                        }
                    }
                }
                if this
                    .update(cx, |view, cx| {
                        view.permissions.unavailable = true;
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                async_io::Timer::after(TIMEOUT).await;
            }
        }));
    }

    pub(super) fn permissions_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let mut parts = vec![self.chip_row("Keyboard shortcut access")];
        let description = "Allowed apps can receive keys Denial uses for its shortcuts, such as Super, while focused.";
        let state = &self.permissions;
        let label = match &state.answers {
            _ if state.unavailable => "Permission store unavailable",
            None => "Loading permissions…",
            Some(answers) if answers.is_empty() => "No app has asked yet",
            Some(_) => "Keyboard shortcuts",
        };
        parts.push(island(&self.look).child(self.row(
            "permissions-status",
            Some(Icon::Shield),
            label,
            Some(say(description)),
            (true, true),
        )));
        let Some(answers) = state.answers.as_ref().filter(|answers| !answers.is_empty()) else {
            return parts;
        };
        let mut apps: Vec<_> = answers
            .iter()
            .map(|(id, answer)| {
                let app = self.installed.apps.iter().find(|app| {
                    app.id
                        .strip_suffix(".desktop")
                        .unwrap_or(&app.id)
                        .eq_ignore_ascii_case(id)
                        || app
                            .startup_wm_class
                            .as_ref()
                            .is_some_and(|class| class.eq_ignore_ascii_case(id))
                });
                (
                    id,
                    *answer,
                    app.map_or(id.as_str(), |app| app.name.as_str()),
                )
            })
            .collect();
        apps.sort_by_cached_key(|(id, _, name)| (name.to_lowercase(), *id));
        parts.push(
            self.rows_island(apps.into_iter(), 0., |(id, answer, name), ends| {
                let row = self
                    .row(
                        SharedString::from(format!("permission:{id}")),
                        None,
                        name.to_owned(),
                        (name != id).then(|| id.clone().into()),
                        ends,
                    )
                    .child(self.value(say(answer.label())))
                    .child(self.chevron());
                let live = layer.live() && !state.unavailable && !state.saving.contains(id);
                let id = id.clone();
                self.live(row, live, cx, move |this, _, cx| {
                    this.choose_permission(id.clone(), answer, cx)
                })
            }),
        );
        parts
    }

    fn choose_permission(&mut self, app: String, answer: Answer, cx: &mut Context<Self>) {
        self.show(
            Dialog::Pick {
                title: "Keyboard shortcut access".into(),
                options: Answer::ALL.map(|choice| choice.label().into()).into(),
                picked: Answer::ALL
                    .iter()
                    .position(|choice| *choice == answer)
                    .unwrap_or(0),
                pick: Rc::new(move |this, choice, _, cx| {
                    if let Some(&choice) = Answer::ALL.get(choice)
                        && choice != answer
                    {
                        this.save_permission(app.clone(), choice, cx);
                    }
                }),
            },
            cx,
        );
    }

    fn save_permission(&mut self, app: String, answer: Answer, cx: &mut Context<Self>) {
        if self.permissions.unavailable || !self.permissions.saving.insert(app.clone()) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = with_timeout(async {
                let bus = Connection::session().await?;
                bus.call_method(
                    Some(SERVICE),
                    PATH,
                    Some(SERVICE),
                    "SetPermission",
                    &(TABLE, true, ID, app.as_str(), [answer.stored()].as_slice()),
                )
                .await
                .map(drop)
            })
            .await;
            let _ = this.update(cx, |view, cx| {
                view.permissions.saving.remove(&app);
                if let Err(error) = result {
                    eprintln!("settings: saving permission: {error}");
                    view.note("Could not save the permission", cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
