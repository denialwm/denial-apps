//! Application environment overrides. Per-app entries use desktop file IDs.
//!
//! In a wide page the applications are listed at the left and the picked
//! one's overrides show beside them; in a narrow one the application is
//! picked in a dialog and the same cards follow it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use denial_look::INSET;
use denial_look::words::fill;
use denial_look_gpui::input::TextInput;
use denial_look_gpui::{Icon, control_fill, control_radius, icon, island, say, solid};
use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, ObjectFit, SharedString, Stateful,
    StyledImage, Window, div, img, prelude::*, px,
};
use serde_json::{Map, Value, json};

use super::parts::{ICON, ROW_GAP, ROW_PAD, TILE};
use super::{Dialog, Layer, Settings};

const ENV: &str = "applicationEnvironment";
const TITLE: &str = "Application environment";
/// A page this wide lists the applications beside the picked one.
const TWO_PANES: f32 = 640.0;
const SCOPES_WIDTH: f32 = 240.0;
/// The list of applications scrolls past this height.
const SCOPES_HEIGHT: f32 = 460.0;
/// A name and a value fit side by side in a card this wide.
const SIDE_BY_SIDE: f32 = 440.0;
/// An application's icon in the list.
const SCOPE_ICON: f32 = 28.0;
/// A small button's side, and a field's height.
const COMPACT: f32 = 36.0;
/// The longest value Denial passes on.
const VALUE_LIMIT: usize = 16 * 1024;

/// What the override being written does.
#[derive(Default)]
pub(super) struct Editor {
    /// The variable is kept from the application, not given a value.
    hide: bool,
    /// The override being changed, when it is not a new one.
    editing: Option<String>,
    /// Why the last attempt to save was refused.
    error: Option<&'static str>,
}

/// An application in the list: its desktop file ID, name, icon, how many
/// overrides it has, and whether it is installed.
struct Scope {
    id: String,
    name: String,
    icon: Option<PathBuf>,
    count: usize,
    installed: bool,
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 256
        && bytes
            .next()
            .is_some_and(|ch| ch == b'_' || ch.is_ascii_alphabetic())
        && bytes.all(|ch| ch == b'_' || ch.is_ascii_alphanumeric())
}

fn valid_id(id: &str) -> bool {
    id.ends_with(".desktop") && id.len() <= 4096 && !id.contains(['/', '\0'])
}

fn default_variables(environment: &Value) -> Option<&Map<String, Value>> {
    let object = environment.as_object()?;
    if object
        .values()
        .all(|value| value.is_null() || value.is_string())
    {
        Some(object)
    } else {
        environment["default"].as_object()
    }
}

fn normalized_environment(document: &mut Value) -> &mut Value {
    let environment = &mut document[ENV];
    if let Some(default) = environment.as_object_mut()
        && default
            .values()
            .all(|value| value.is_null() || value.is_string())
    {
        *environment = json!({"default": std::mem::take(default), "applications": {}});
    }
    if !environment.is_object() {
        *environment = json!({});
    }
    environment
}

fn change_variable(document: &mut Value, scope: Option<&str>, name: &str, value: Option<&Value>) {
    let environment = normalized_environment(document);
    let variables = match scope {
        Some(id) => {
            if !environment["applications"].is_object() {
                environment["applications"] = json!({});
            }
            &mut environment["applications"][id]
        }
        None => &mut environment["default"],
    };
    if !variables.is_object() {
        *variables = json!({});
    }
    let variables = variables.as_object_mut().unwrap();
    match value {
        Some(value) => {
            variables.insert(name.to_owned(), value.clone());
        }
        None => {
            variables.remove(name);
        }
    }
    if variables.is_empty()
        && let Some(id) = scope
    {
        environment["applications"]
            .as_object_mut()
            .unwrap()
            .remove(id);
    }
}

impl Settings {
    pub(super) fn environment_page(
        &self,
        width: f32,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let scope = self.environment_scope.as_deref();
        let live = layer.live() && self.denial.document.is_some();
        let environment = self.kept(ENV).unwrap_or(&Value::Null);
        let global = default_variables(environment);
        let overrides = match scope {
            Some(id) => environment["applications"][id].as_object(),
            None => global,
        };
        let two_panes = width >= TWO_PANES;
        let detail_width = if two_panes {
            width - SCOPES_WIDTH - 3.0 * INSET
        } else {
            width - 2.0 * INSET
        };
        let detail = [
            self.environment_header(overrides, global, live, cx),
            self.environment_editor(detail_width, live, cx),
            self.environment_overrides(overrides, global, live, cx),
        ];

        let mut parts = vec![self.environment_note()];
        if two_panes {
            parts.push(
                div()
                    .flex()
                    .items_start()
                    .gap(px(INSET))
                    .child(self.environment_scopes(environment, live, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(INSET))
                            .children(detail),
                    ),
            );
        } else {
            parts.push(self.environment_scope_rows(live, cx));
            parts.extend(detail);
        }

        let reset = self.row(
            "environment-reset",
            None,
            "Reset all environment overrides",
            None,
            (true, true),
        );
        let has_overrides = global.is_some_and(|map| !map.is_empty())
            || environment["applications"]
                .as_object()
                .is_some_and(|map| !map.is_empty());
        parts.push(island(&self.look).child(self.live(
            reset,
            live && has_overrides,
            cx,
            |this, _, cx| this.confirm_environment_reset(true, cx),
        )));
        parts
    }

    /// What the overrides reach, said once over the page.
    fn environment_note(&self) -> Div {
        island(&self.look).child(
            div()
                .p(px(ROW_PAD))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(icon(Icon::Info, px(20.), solid(self.look.accent_text)))
                        .child(self.heading("Launch scope")),
                )
                .child(self.explained(
                    "These overrides apply only to applications and shortcut commands started \
                     by Denial. They affect subsequent launches, not XDG autostart entries, \
                     systemd services, D-Bus services, or Denial itself.",
                )),
        )
    }

    /// A card's title.
    fn heading(&self, title: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::SEMIBOLD)
            .child(say(title))
    }

    /// A line or two that explains, in the secondary colour.
    fn explained(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(13.))
            .text_color(solid(self.look.palette.secondary))
            .child(say(text))
    }

    /// The applications that can have overrides: those installed, and
    /// those that have overrides and are gone. The ones with overrides
    /// come first.
    fn scopes(&self, environment: &Value) -> Vec<Scope> {
        let configured = environment["applications"].as_object();
        let count = |id: &str| {
            configured
                .and_then(|scopes| scopes.get(id)?.as_object())
                .map_or(0, Map::len)
        };
        let mut scopes: BTreeMap<String, Scope> = self
            .installed
            .apps
            .iter()
            .filter(|app| valid_id(&app.id))
            .map(|app| {
                let scope = Scope {
                    id: app.id.clone(),
                    name: app.name.clone(),
                    icon: app.icon.clone(),
                    count: count(&app.id),
                    installed: true,
                };
                (app.id.clone(), scope)
            })
            .collect();
        for id in configured.into_iter().flat_map(Map::keys) {
            if valid_id(id) && !scopes.contains_key(id) {
                let scope = Scope {
                    id: id.clone(),
                    name: say("Unavailable application").to_string(),
                    icon: None,
                    count: count(id),
                    installed: false,
                };
                scopes.insert(id.clone(), scope);
            }
        }
        let mut scopes: Vec<Scope> = scopes.into_values().collect();
        scopes.sort_by_cached_key(|scope| (scope.count == 0, scope.name.to_lowercase()));
        scopes
    }

    /// The applications, with a field that narrows them: the wide page's
    /// left pane.
    fn environment_scopes(&self, environment: &Value, live: bool, cx: &mut Context<Self>) -> Div {
        let query = self
            .environment_search
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        let has = |scope: &Scope| {
            query.is_empty()
                || scope.name.to_lowercase().contains(&query)
                || scope.id.to_lowercase().contains(&query)
        };
        let all = default_variables(environment).map_or(0, Map::len);
        let picked = self.environment_scope.as_deref();
        let mut tiles = vec![self.scope_tile(
            None,
            say("All applications"),
            say("Default launch environment"),
            self.tile(Icon::Copy, SCOPE_ICON).into_any_element(),
            all,
            picked.is_none(),
            live,
            cx,
        )];
        for scope in self.scopes(environment).into_iter().filter(has) {
            let picture = match &scope.icon {
                Some(path) => img(path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None if scope.installed => self.tile(Icon::Apps, SCOPE_ICON).into_any_element(),
                None => self.tile(Icon::Block, SCOPE_ICON).into_any_element(),
            };
            let selected = picked == Some(scope.id.as_str());
            tiles.push(self.scope_tile(
                Some(scope.id.clone()),
                scope.name.into(),
                scope.id.into(),
                picture,
                scope.count,
                selected,
                live,
                cx,
            ));
        }
        island(&self.look)
            .w(px(SCOPES_WIDTH))
            .flex_none()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.caption("Applications").px(px(6.)).pt(px(2.)))
            .child(
                self.boxed(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon(
                            Icon::Search,
                            px(18.),
                            solid(self.look.palette.secondary),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.environment_search.clone()),
                        ),
                ),
            )
            .child(
                div()
                    .id("environment-scopes")
                    .max_h(px(SCOPES_HEIGHT))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(tiles),
            )
    }

    /// One application of the list; `id` is None for all of them.
    #[expect(clippy::too_many_arguments)]
    fn scope_tile(
        &self,
        id: Option<String>,
        name: SharedString,
        detail: SharedString,
        picture: AnyElement,
        count: usize,
        selected: bool,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = id.as_deref().unwrap_or("all");
        let tile = div()
            .id(SharedString::from(format!("environment-scope:{key}")))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(8.))
            .py(px(6.))
            .rounded(control_radius(&self.look))
            .when(selected, |tile| tile.bg(control_fill(&self.look)))
            .child(div().flex_none().size(px(SCOPE_ICON)).child(picture))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(solid(self.look.palette.secondary))
                            .child(detail),
                    ),
            )
            .children((count > 0).then(|| {
                div()
                    .flex_none()
                    .px(px(7.))
                    .rounded_full()
                    .bg(control_fill(&self.look))
                    .text_size(px(12.))
                    .child(count.to_string())
            }));
        self.live(tile, live && !selected, cx, move |this, _, cx| {
            this.environment_scope = id.clone();
            this.forget_environment_edit(cx);
        })
    }

    /// The narrow page's way to the applications: the picked one, opening
    /// the list, and a search over them.
    fn environment_scope_rows(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let name = match self.environment_scope.as_deref() {
            Some(id) => self
                .installed
                .apps
                .iter()
                .find(|app| app.id == id)
                .map_or(id, |app| app.name.as_str())
                .to_owned(),
            None => say("All applications").to_string(),
        };
        let selection = self
            .row(
                "environment-scope",
                None,
                "Application scope",
                None,
                (true, false),
            )
            .child(self.value(name))
            .child(self.chevron());
        let search = self
            .row(
                "environment-search",
                None,
                "Search application scopes",
                None,
                (false, true),
            )
            .child(self.chevron());
        island(&self.look)
            .child(self.live(selection, live, cx, |this, _, cx| this.choose_scope("", cx)))
            .child(self.separator(0.))
            .child(self.live(search, live, cx, |this, window, cx| {
                this.output_field
                    .update(cx, |field, cx| field.set_text(String::new(), cx));
                this.show(
                    Dialog::TextEdit {
                        title: "Search application scopes",
                        detail: "Enter an application name or desktop file ID.",
                        valid: Rc::new(|_| true),
                        save: Rc::new(|this, text, cx| this.choose_scope(&text, cx)),
                    },
                    cx,
                );
                window.focus(&this.output_field.focus_handle(cx), cx);
            }))
    }

    /// The picked application: its icon and name, what it inherits, and a
    /// button that clears its overrides.
    fn environment_header(
        &self,
        overrides: Option<&Map<String, Value>>,
        global: Option<&Map<String, Value>>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let app = self
            .environment_scope
            .as_deref()
            .map(|id| (id, self.installed.apps.iter().find(|app| app.id == id)));
        let (picture, name, detail): (AnyElement, SharedString, SharedString) = match app {
            None => (
                self.tile(Icon::Copy, TILE).into_any_element(),
                say("All applications"),
                say("Default launch environment"),
            ),
            Some((id, found)) => {
                let picture = match found.and_then(|app| app.icon.clone()) {
                    Some(path) => img(path)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element(),
                    None if found.is_some() => self.tile(Icon::Apps, TILE).into_any_element(),
                    None => self.tile(Icon::Block, TILE).into_any_element(),
                };
                let name = match found {
                    Some(app) => app.name.clone().into(),
                    None => say("Unavailable application"),
                };
                let inherited = global.map_or(0, Map::len);
                let detail = if inherited > 0 {
                    let rules = fill(
                        "Inherits {count} all-app rules",
                        &[("count", &inherited.to_string())],
                    );
                    format!("{id} · {rules}").into()
                } else {
                    id.to_owned().into()
                };
                (picture, name, detail)
            }
        };
        let clear = overrides.is_some_and(|map| !map.is_empty()).then(|| {
            self.compact_button(
                "environment-clear",
                "Clear",
                false,
                live,
                cx,
                |this, _, cx| this.confirm_environment_reset(false, cx),
            )
        });
        island(&self.look).child(
            div()
                .flex()
                .items_center()
                .gap(px(ROW_GAP))
                .px(px(ROW_PAD))
                .py(px(12.))
                .child(div().flex_none().size(px(TILE)).child(picture))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .text_color(solid(self.look.palette.secondary))
                                .child(detail),
                        ),
                )
                .children(clear),
        )
    }

    /// A button as wide as its label: a card's own action.
    fn compact_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        accent: bool,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let button = div()
            .id(id)
            .flex_none()
            .h(px(COMPACT))
            .px(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .font_weight(FontWeight::SEMIBOLD)
            .child(say(label));
        let button = if accent {
            button
                .bg(solid(self.look.accent))
                .text_color(solid(self.look.on_accent))
        } else {
            button.bg(control_fill(&self.look))
        };
        if live {
            self.tappable(button, cx, action)
        } else {
            button.opacity(0.5)
        }
    }

    /// A small icon button at a row's end.
    fn compact_icon(
        &self,
        id: String,
        glyph: Icon,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let button = div()
            .id(SharedString::from(id))
            .flex_none()
            .size(px(COMPACT))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .child(icon(glyph, px(18.), solid(self.look.palette.secondary)));
        self.live(button, live, cx, action)
    }

    /// A field's box, as high as a small button.
    fn boxed(&self, content: Div) -> Div {
        div()
            .h(px(COMPACT + 4.))
            .flex()
            .flex_col()
            .justify_center()
            .px(px(11.))
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .child(content)
    }

    /// A field under its label.
    fn labelled_field(&self, label: &'static str, content: Div) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(solid(self.look.palette.secondary))
                    .child(say(label)),
            )
            .child(self.boxed(content))
    }

    /// The card that writes an override: whether it gives the variable a
    /// value or hides it, the name, the value, and the button that saves.
    fn environment_editor(&self, width: f32, live: bool, cx: &mut Context<Self>) -> Div {
        let editor = &self.environment_editor;
        let hide = editor.hide;
        let title: SharedString = match &editor.editing {
            Some(name) => fill("Edit {variable}", &[("variable", name)]).into(),
            None => say("Add an override"),
        };
        let modes = self.segmented(
            &["Add variable", "Hide variable"],
            usize::from(hide),
            live,
            cx,
            Rc::new(|this, option, _, cx| {
                this.environment_editor.hide = option == 1;
                this.environment_editor.error = None;
                cx.notify();
            }),
        );
        let explained = if hide {
            "Strip an inherited variable immediately before exec. The application behaves as \
             if that name was never exported."
        } else {
            "Inject a literal value when Denial executes an application. An empty value is \
             preserved as an empty string."
        };
        // The name of an override being changed stays as it is.
        let name = match &editor.editing {
            Some(name) => div()
                .truncate()
                .text_color(solid(self.look.palette.secondary))
                .child(name.clone()),
            None => div().child(self.environment_name.clone()),
        };
        let name = self.labelled_field("Variable name", name);
        let value = (!hide)
            .then(|| self.labelled_field("Value", div().child(self.environment_value.clone())));
        let fields = if width - 2.0 * ROW_PAD >= SIDE_BY_SIDE {
            div()
                .flex()
                .gap(px(10.))
                .child(name.flex_1().min_w_0())
                .children(value.map(|value| value.flex_1().min_w_0()))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(name)
                .children(value)
        };
        let save = match (&editor.editing, hide) {
            (Some(_), _) => "Update variable",
            (None, false) => "Add",
            (None, true) => "Hide",
        };
        let cancel = editor.editing.is_some().then(|| {
            self.compact_button(
                "environment-cancel",
                "Cancel",
                false,
                live,
                cx,
                |this, _, cx| this.forget_environment_edit(cx),
            )
        });
        island(&self.look).child(
            div()
                .p(px(ROW_PAD))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(div().flex().child(self.heading(title)))
                .child(modes)
                .child(self.explained(explained).text_center())
                .child(fields)
                .children(editor.error.map(|error| {
                    div()
                        .text_size(px(13.))
                        .text_color(solid(self.look.palette.error))
                        .child(say(error))
                }))
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .gap(px(8.))
                        .child(self.compact_button(
                            "environment-save",
                            save,
                            true,
                            live,
                            cx,
                            |this, window, cx| this.save_environment_variable(window, cx),
                        ))
                        .children(cancel),
                ),
        )
    }

    /// The picked application's overrides, each with what it does and the
    /// buttons that change and remove it.
    fn environment_overrides(
        &self,
        overrides: Option<&Map<String, Value>>,
        global: Option<&Map<String, Value>>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let per_application = self.environment_scope.is_some();
        let variables: BTreeMap<&String, &Value> = overrides.into_iter().flatten().collect();
        let count = variables.len();
        let card = island(&self.look).child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(ROW_PAD))
                .pt(px(14.))
                .pb(px(8.))
                .child(self.heading("Configured overrides"))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(solid(self.look.palette.secondary))
                        .child(fill("{count} configured", &[("count", &count.to_string())])),
                ),
        );
        if variables.is_empty() {
            return card.child(
                div()
                    .px(px(ROW_PAD))
                    .pt(px(14.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(icon(
                        Icon::Code,
                        px(26.),
                        solid(self.look.palette.secondary),
                    ))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(say("No overrides")),
                    )
                    .child(
                        self.explained(
                            "Add a variable above to change the environment of subsequently \
                             launched applications.",
                        )
                        .text_center(),
                    ),
            );
        }
        variables.into_iter().fold(card, |card, (name, value)| {
            let hidden = value.is_null();
            let does: SharedString = match value.as_str() {
                None => say("Removed from child environment"),
                Some("") => say("Empty string"),
                Some(value) => value.to_owned().into(),
            };
            let over = per_application && global.is_some_and(|global| global.contains_key(name));
            let does: SharedString = if over {
                format!("{does} · {}", say("Overrides the all-applications rule")).into()
            } else {
                does
            };
            let mark = if hidden {
                icon(Icon::Minus, px(ICON - 4.), solid(self.look.palette.error))
            } else {
                icon(Icon::Plus, px(ICON - 4.), solid(self.look.accent_text))
            };
            let (edited, removed) = (name.clone(), name.clone());
            let shown = value.as_str().map(str::to_owned);
            card.child(self.separator(0.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(ROW_PAD))
                    .pr(px(8.))
                    .py(px(6.))
                    .child(div().flex_none().child(mark))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(name.clone()))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(solid(self.look.palette.secondary))
                                    .child(does),
                            ),
                    )
                    .child(self.compact_icon(
                        format!("environment-edit:{name}"),
                        Icon::New,
                        live,
                        cx,
                        move |this, window, cx| {
                            this.edit_environment_variable(&edited, shown.as_deref(), window, cx)
                        },
                    ))
                    .child(self.compact_icon(
                        format!("environment-remove:{name}"),
                        Icon::Close,
                        live,
                        cx,
                        move |this, _, cx| this.remove_environment_variable(&removed, cx),
                    )),
            )
        })
    }

    fn choose_scope(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_lowercase();
        let scopes: Vec<Scope> = self
            .scopes(self.kept(ENV).unwrap_or(&Value::Null))
            .into_iter()
            .filter(|scope| {
                scope.id.to_lowercase().contains(&query)
                    || scope.name.to_lowercase().contains(&query)
            })
            .collect();
        if !query.is_empty() && scopes.is_empty() {
            self.note("No matching applications", cx);
            return;
        }
        let mut ids = vec![None];
        let mut options = vec![say("All applications")];
        for scope in scopes {
            options.push(if scope.installed {
                format!("{} · {}", scope.name, scope.id).into()
            } else {
                scope.id.clone().into()
            });
            ids.push(Some(scope.id));
        }
        let picked = ids
            .iter()
            .position(|scope| *scope == self.environment_scope)
            .unwrap_or(ids.len());
        self.show(
            Dialog::Pick {
                title: "Application scope".into(),
                options,
                picked,
                pick: Rc::new(move |this, index, _, cx| {
                    if let Some(scope) = ids.get(index) {
                        this.environment_scope = scope.clone();
                        this.forget_environment_edit(cx);
                    }
                }),
            },
            cx,
        );
    }

    /// Empties the card that writes an override.
    fn forget_environment_edit(&mut self, cx: &mut Context<Self>) {
        self.environment_editor = Editor::default();
        for field in [&self.environment_name, &self.environment_value] {
            field.update(cx, |field: &mut TextInput, cx| field.clear(cx));
        }
        cx.notify();
    }

    /// The name field was typed in, or the value field: what was refused
    /// is no longer what is there.
    pub(super) fn typed_environment(&mut self, cx: &mut Context<Self>) {
        self.environment_editor.error = None;
        cx.notify();
    }

    /// Enter in the name field: on to the value, or saves when there is
    /// none to give.
    pub(super) fn submit_environment_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.environment_editor.hide {
            self.save_environment_variable(window, cx);
        } else {
            window.focus(&self.environment_value.focus_handle(cx), cx);
        }
    }

    /// Saves what the card holds, or says why it cannot.
    pub(super) fn save_environment_variable(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editing = self.environment_editor.editing.clone();
        let hide = self.environment_editor.hide;
        let name = editing
            .clone()
            .unwrap_or_else(|| self.environment_name.read(cx).text().trim().to_owned());
        let value = self.environment_value.read(cx).text().to_owned();
        let scope = self.environment_scope.clone();
        let taken = editing.is_none() && {
            let environment = self.kept(ENV).unwrap_or(&Value::Null);
            let overrides = match scope.as_deref() {
                Some(id) => environment["applications"][id].as_object(),
                None => default_variables(environment),
            };
            overrides.is_some_and(|overrides| overrides.contains_key(&name))
        };
        let error = if name.is_empty() {
            Some("Enter a variable name.")
        } else if !valid_name(&name) {
            Some("Use letters, numbers, and underscores, beginning with a letter or underscore.")
        } else if taken {
            Some("That variable already has an override.")
        } else if !hide && value.contains('\0') {
            Some("Values cannot contain a NUL character.")
        } else if !hide && value.len() > VALUE_LIMIT {
            Some("The value is too long.")
        } else {
            None
        };
        if let Some(error) = error {
            self.environment_editor.error = Some(error);
            cx.notify();
            return;
        }
        let stored = if hide { Value::Null } else { json!(value) };
        self.change(
            TITLE,
            move |document| change_variable(document, scope.as_deref(), &name, Some(&stored)),
            cx,
        );
        self.forget_environment_edit(cx);
        // With a pointer the next one is typed at once; on a phone the
        // keyboard would come back up.
        if self.wide {
            window.focus(&self.environment_name.focus_handle(cx), cx);
        }
    }

    /// Puts the override `name` in the card, to change it.
    fn edit_environment_variable(
        &mut self,
        name: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.environment_editor = Editor {
            hide: value.is_none(),
            editing: Some(name.to_owned()),
            error: None,
        };
        let text = value.unwrap_or_default().to_owned();
        self.environment_value
            .update(cx, |field, cx| field.set_text(text, cx));
        if value.is_some() {
            window.focus(&self.environment_value.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn remove_environment_variable(&mut self, name: &str, cx: &mut Context<Self>) {
        let scope = self.environment_scope.clone();
        if self.environment_editor.editing.as_deref() == Some(name) {
            self.forget_environment_edit(cx);
        }
        let name = name.to_owned();
        self.change(
            TITLE,
            move |document| change_variable(document, scope.as_deref(), &name, None),
            cx,
        );
    }

    fn confirm_environment_reset(&mut self, all: bool, cx: &mut Context<Self>) {
        let scope = self.environment_scope.clone();
        let title = if all {
            "Reset all environment overrides"
        } else {
            "Clear scope overrides"
        };
        self.show(
            Dialog::Confirm {
                title: title.into(),
                consequence: "Changes apply the next time Denial launches an app.".into(),
                action: "Reset",
                confirm: Rc::new(move |this, _, cx| {
                    let scope = scope.clone();
                    this.change(
                        title,
                        move |document| {
                            let environment = normalized_environment(document);
                            if all {
                                *environment = json!({"default": {}, "applications": {}});
                            } else if let Some(id) = &scope {
                                if let Some(applications) =
                                    environment["applications"].as_object_mut()
                                {
                                    applications.remove(id);
                                }
                            } else {
                                environment["default"] = json!({});
                            }
                        },
                        cx,
                    );
                }),
            },
            cx,
        );
    }
}
