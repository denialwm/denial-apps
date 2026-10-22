//! Pieces every screen is built from: the placing of islands, presses and
//! buttons, icon tiles, rows, section chips and the top group's contents.

use denial_look::mobile::TOUCH;
use denial_look::{CONTROL_FILL, PRESSED_FILL, lerp_rgb, on, palette};
use denial_look_gpui::{Icon, chip, color, control_fill, control_radius, dp, icon, island, solid};
use gpui::{
    ClickEvent, Context, Div, Entity, FontWeight, SharedString, Stateful, Window, div, prelude::*,
    px,
};

use super::{Back, Layer, Motion, Settings};
use denial_look_gpui::input::TextInput;

/// Size of an icon in a button, in dp.
pub(super) const ICON: f32 = 22.0;
/// A row's padding from its island's edge, in dp.
pub(super) const ROW_PAD: f32 = 16.0;
/// Between a row's icon tile and its text, in dp.
pub(super) const ROW_GAP: f32 = 14.0;
/// A setting's icon tile, in dp.
pub(super) const TILE: f32 = 33.0;

/// What a button looks like.
#[derive(Clone, Copy)]
pub(super) enum Fill {
    /// The control fill inside an island: the foreground, faint.
    Control,
    /// The control fill, for an action that destroys something: it reads in
    /// the error colour.
    Danger,
    Accent,
    /// The confirm button for something that cannot be undone.
    Destructive,
}

/// Places an island as `motion` says. It scrolls under the top group, the
/// gesture zone and the keyboard as it is, never faded.
pub(super) fn place(element: impl IntoElement, motion: Motion) -> Div {
    div()
        .relative()
        .top(px(motion.offset))
        .opacity(motion.opacity)
        .child(element)
}

impl Settings {
    /// Shows the press on `element` and runs `action` on a tap. While a
    /// dialog is up, a tap on the page under it only closes the dialog.
    pub(super) fn tappable(
        &self,
        element: Stateful<Div>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.pressable(element, cx, move |this, window, cx| {
            if this.dialog.is_some() {
                return this.dismiss(window, cx);
            }
            action(this, window, cx)
        })
    }

    /// Shows the press on `element` and runs `action` on a tap: for the
    /// dialog's own controls.
    pub(super) fn pressable(
        &self,
        element: Stateful<Div>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let pressed = color(self.look.palette.foreground, PRESSED_FILL);
        element
            .cursor_pointer()
            .active(move |s| s.bg(pressed))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                action(this, window, cx)
            }))
    }

    /// A clear icon button in the top group.
    pub(super) fn icon_button(
        &self,
        id: &'static str,
        glyph: Icon,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let button = div()
            .id(id)
            .flex_none()
            .size(dp(TOUCH))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .child(icon(glyph, dp(ICON), solid(self.look.palette.foreground)));
        if live {
            self.tappable(button, cx, action)
        } else {
            button
        }
    }

    pub(super) fn back_button(&self, layer: Layer, cx: &mut Context<Self>) -> Stateful<Div> {
        self.icon_button("back", Icon::Back, layer.live(), cx, |this, window, cx| {
            this.back(&Back, window, cx)
        })
    }

    /// The row inside the top group.
    pub(super) fn top_row(&self) -> Div {
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(dp(8.))
            .px(dp(4.))
    }

    pub(super) fn title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(dp(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title.into())
    }

    /// A text field in the top group, `lead` before it.
    pub(super) fn field(&self, lead: impl IntoElement, input: &Entity<TextInput>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .h(dp(TOUCH))
            .flex()
            .items_center()
            .gap(dp(8.))
            .px(dp(11.))
            .rounded(control_radius(&self.look))
            .bg(color(self.look.palette.foreground, CONTROL_FILL))
            .text_color(solid(self.look.palette.secondary))
            .child(div().flex_none().child(lead))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(solid(self.look.palette.foreground))
                    .child(input.clone()),
            )
    }

    /// A chip that names the island under it, on the frost.
    pub(super) fn chip_row(&self, label: impl Into<SharedString>) -> Div {
        div()
            .pt(dp(6.))
            .flex()
            .child(chip(&self.look).child(label.into()))
    }

    /// An island of rows under its label, the label inside it in accent
    /// text.
    pub(super) fn labelled(&self, label: impl Into<SharedString>) -> Div {
        island(&self.look).child(
            div()
                .px(dp(ROW_PAD))
                .pt(dp(12.))
                .pb(dp(2.))
                .text_size(dp(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(self.look.accent_text))
                .child(label.into().to_uppercase()),
        )
    }

    /// The hairline between two rows, from `indent` dp in.
    pub(super) fn separator(&self, indent: f32) -> Div {
        div().h(px(1.)).ml(dp(indent)).bg(color(
            self.look.palette.separator,
            self.look.palette.separator_alpha,
        ))
    }

    /// An icon on a square of the control fill, `size` dp.
    pub(super) fn tile(&self, glyph: Icon, size: f32) -> Div {
        div()
            .flex_none()
            .size(dp(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .child(icon(
                glyph,
                dp(size * 0.58),
                solid(self.look.palette.foreground),
            ))
    }

    /// A title over the line that says more, in the secondary colour.
    pub(super) fn text_column(
        &self,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
    ) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(dp(2.))
            .child(div().child(title.into()))
            .children(detail.map(|detail| {
                div()
                    .text_size(dp(14.))
                    .text_color(solid(self.look.palette.secondary))
                    .child(detail)
            }))
    }

    /// A row that leads to a page: its tile, its title and what it holds.
    /// Rounded at the bottom when `last`, as its island is.
    pub(super) fn nav_row(
        &self,
        id: impl Into<gpui::ElementId>,
        glyph: Icon,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        last: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .px(dp(ROW_PAD))
            .py(dp(12.))
            .when(last, |row| row.rounded_b(px(self.look.group_radius())))
            .child(self.tile(glyph, 40.))
            .child(self.text_column(title, Some(detail.into())))
    }

    /// The opaque colour of `fill`; none for a control, which takes the tint
    /// of the island under it.
    fn fill(&self, fill: Fill) -> Option<u32> {
        match fill {
            Fill::Control | Fill::Danger => None,
            Fill::Accent => Some(self.look.accent),
            Fill::Destructive => Some(palette::DESTRUCTIVE),
        }
    }

    /// A labelled button inside an island, taking `rest` of a row's width
    /// when it shares one. A button that is not `live` shows faint and does
    /// nothing.
    pub(super) fn button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        fill: Fill,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let foreground = self.look.palette.foreground;
        let ink = match (self.fill(fill), fill) {
            (Some(fill), _) if fill == self.look.accent => self.look.on_accent,
            (Some(fill), _) => on(fill),
            (None, Fill::Danger) => self.look.palette.error,
            (None, _) => foreground,
        };
        let button = div()
            .id(id)
            .flex_1()
            .min_w_0()
            .h(dp(TOUCH))
            .px(dp(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .truncate()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(ink))
            .child(label.into());
        let button = match self.fill(fill) {
            Some(fill) => button.bg(solid(fill)),
            None => button.bg(control_fill(&self.look)),
        };
        if !live {
            return button.opacity(0.5);
        }
        let pressed = match self.fill(fill) {
            Some(fill) => solid(lerp_rgb(fill, foreground, PRESSED_FILL)),
            None => color(foreground, PRESSED_FILL),
        };
        button
            .cursor_pointer()
            .active(move |s| s.bg(pressed))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                action(this, window, cx)
            }))
    }

    /// Buttons side by side, as wide as their row.
    pub(super) fn buttons(&self, buttons: impl IntoIterator<Item = Stateful<Div>>) -> Div {
        div().flex().gap(dp(10.)).children(buttons)
    }

    /// A row of an island: an icon tile when there is `glyph`, the title and
    /// the line under it, and `end` at its end. Rounded as its island is when
    /// `first` or `last`, so its press fills the island's corner.
    pub(super) fn row(
        &self,
        id: impl Into<gpui::ElementId>,
        glyph: Option<Icon>,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
        (first, last): (bool, bool),
    ) -> Stateful<Div> {
        let radius = px(self.look.group_radius());
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .px(dp(ROW_PAD))
            .py(dp(12.))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
            .children(glyph.map(|glyph| self.tile(glyph, TILE)))
            .child(self.text_column(title, detail))
    }

    /// `rows` in an island, a hairline between each two from `indent` dp in.
    /// Each row is told whether it is first and last.
    pub(super) fn rows_island<T>(
        &self,
        items: impl ExactSizeIterator<Item = T>,
        indent: f32,
        mut row: impl FnMut(T, (bool, bool)) -> Stateful<Div>,
    ) -> Div {
        let count = items.len();
        items
            .enumerate()
            .fold(island(&self.look), |island, (ix, item)| {
                island
                    .when(ix > 0, |island| island.child(self.separator(indent)))
                    .child(row(item, (ix == 0, ix + 1 == count)))
            })
    }

    /// A value at a row's end, in the secondary colour.
    pub(super) fn value(&self, value: impl Into<SharedString>) -> Div {
        div()
            .flex_none()
            .max_w(gpui::relative(0.55))
            .truncate()
            .text_color(solid(self.look.palette.secondary))
            .child(value.into())
    }

    /// The chevron at the end of a row that opens something.
    pub(super) fn chevron(&self) -> gpui::Svg {
        icon(Icon::Chevron, dp(20.), solid(self.look.palette.secondary))
    }

    /// An on or off switch.
    pub(super) fn toggle(&self, on: bool) -> Div {
        let (track, knob) = if on {
            (solid(self.look.accent), self.look.on_accent)
        } else {
            (control_fill(&self.look), self.look.palette.secondary)
        };
        div()
            .flex_none()
            .w(dp(45.))
            .h(dp(26.))
            .p(dp(3.))
            .flex()
            .when(on, |toggle| toggle.justify_end())
            .rounded_full()
            .bg(track)
            .child(div().size(dp(20.)).rounded_full().bg(solid(knob)))
    }

    /// `element` with its press and `action`, on the shown screen.
    pub(super) fn live(
        &self,
        element: Stateful<Div>,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        if live {
            self.tappable(element, cx, action)
        } else {
            element
        }
    }
}
