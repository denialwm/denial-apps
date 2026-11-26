//! Pieces every screen is built from: the placing of islands, presses and
//! buttons, icon tiles, rows, section chips and the top group's contents.

use denial_look::mobile::TOUCH;
use denial_look::{
    CONTROL_FILL, HOVER_FILL, PRESSED_FILL, decelerate, floating, lerp_rgb, motion, nested_radius,
    on, palette,
};
use denial_look_gpui::{Icon, chip, color, control_fill, control_radius, icon, island, say, solid};
use gpui::{
    AnimationExt, AnyElement, ClickEvent, Context, Div, ElementId, Entity, FontWeight,
    SharedString, SpringAnimation, SpringConfig, Stateful, Window, div, prelude::*, px,
};

use super::{Back, Layer, Motion, Settings};
use denial_look_gpui::input::TextInput;

/// Size of an icon in a button.
pub(super) const ICON: f32 = 22.0;
/// A row's padding from its island's edge.
pub(super) const ROW_PAD: f32 = 16.0;
/// Between a row's icon tile and its text.
pub(super) const ROW_GAP: f32 = 14.0;
/// A setting's icon tile.
pub(super) const TILE: f32 = 33.0;
/// Between an opened card's edges and the panels it holds, and between
/// those.
pub(super) const CARD_INSET: f32 = 8.0;
/// A panel inside a card, over the card's own fill.
const PANEL_FILL: f32 = 0.05;

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

/// What an icon button does, shown under the pointer.
struct Hint {
    said: &'static str,
    background: gpui::Hsla,
    text: gpui::Hsla,
}

impl gpui::Render for Hint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .bg(self.background)
            .text_color(self.text)
            .text_size(px(12.))
            .child(say(self.said))
    }
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

    /// `element` with the fill a row takes under the pointer, in the wide
    /// layout: a phone has no pointer, and a finger would leave it lit.
    pub(super) fn hovered(&self, element: Stateful<Div>) -> Stateful<Div> {
        let fill = color(self.look.palette.foreground, HOVER_FILL);
        element.when(self.wide, |element| element.hover(move |s| s.bg(fill)))
    }

    /// The spring a control moves on: it settles in about 200 ms without
    /// passing its end, on Denial's duration scale. None with reduced
    /// motion.
    fn spring(&self) -> Option<SpringConfig> {
        let scale = self.look.duration_scale;
        (scale > 0.0).then(|| SpringConfig::new(900. / (scale * scale), 60. / scale, 1.))
    }

    /// What `draw` makes of 1 when `on` and of 0 when not, moving between
    /// the two when `on` changes. Where it first shows it is already there.
    /// `id` names it among what its row holds.
    pub(super) fn eased(
        &self,
        id: impl Into<ElementId>,
        on: bool,
        draw: impl FnOnce(f32) -> Div + 'static,
    ) -> AnyElement {
        self.sprung(id, if on { 1.0 } else { 0.0 }, move |at| {
            draw(at.clamp(0.0, 1.0))
        })
    }

    /// What `draw` makes of `target`, moving there on the spring when
    /// `target` changes. Where it first shows it is already there.
    pub(super) fn sprung(
        &self,
        id: impl Into<ElementId>,
        target: f32,
        draw: impl FnOnce(f32) -> Div + 'static,
    ) -> AnyElement {
        match self.spring() {
            None => draw(target).into_any_element(),
            Some(spring) => div()
                .with_spring(
                    id,
                    SpringAnimation::new(spring).to(target),
                    move |_, at: f32| draw(at),
                )
                .into_any_element(),
        }
    }

    /// `element` fading in where it first shows: what a row opens. The
    /// fade starts when Settings first builds it, not when it is first
    /// drawn, so a scroll view that leaves it out a while does not replay it.
    pub(super) fn appearing(&self, id: impl Into<ElementId>, element: Div) -> AnyElement {
        let seconds = motion::CHIP_FADE as f32 * self.look.duration_scale / 1000.0;
        let mut appeared = self.appeared.borrow_mut();
        let age = appeared.age(id.into(), std::time::Instant::now());
        let shown = if seconds <= 0.0 {
            1.0
        } else {
            (age.as_secs_f32() / seconds).min(1.0)
        };
        if shown >= 1.0 {
            return element.into_any_element();
        }
        appeared.fading = true;
        element.opacity(decelerate(shown)).into_any_element()
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
            .size(px(TOUCH))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .child(icon(glyph, px(ICON), solid(self.look.palette.foreground)));
        if live {
            self.hovered(self.tappable(button, cx, action))
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
            .gap(px(8.))
            .px(px(4.))
    }

    pub(super) fn title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(say(title))
    }

    /// A text field in the top group, `lead` before it.
    pub(super) fn field(&self, lead: impl IntoElement, input: &Entity<TextInput>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .h(px(TOUCH))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(11.))
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

    /// What names the island under it. Where the list is the whole window
    /// it is a chip on the frost; beside the list it is a heading over the
    /// island, in line with the rows' text and written as the labels inside
    /// islands are.
    pub(super) fn chip_row(&self, label: impl Into<SharedString>) -> Div {
        if self.wide {
            self.caption(label).pl(px(ROW_PAD)).pt(px(12.))
        } else {
            self.notice_row(label)
        }
    }

    /// A section's name with what is done to the whole section at the
    /// line's end: small icon buttons, made by `action`.
    pub(super) fn heading_row(
        &self,
        label: impl Into<SharedString>,
        actions: impl IntoIterator<Item = AnyElement>,
    ) -> Div {
        div()
            .flex()
            .items_end()
            .gap(px(8.))
            .child(self.chip_row(label).flex_1().min_w_0())
            .child(div().flex_none().flex().gap(px(6.)).children(actions))
    }

    /// An icon button for a section's own action, at its heading's end.
    /// `hint` shows under the pointer and names it to a screen reader. The
    /// one that carries a change out is `accent`. One that is not `live`
    /// shows faint and does nothing.
    #[expect(clippy::too_many_arguments)]
    pub(super) fn action(
        &self,
        id: &'static str,
        glyph: Icon,
        hint: &'static str,
        accent: bool,
        live: bool,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        // Beside the list a pointer picks it; where the list is the whole
        // window a finger does, and it is glass on the frost as a chip is.
        let size = if self.wide { 30.0 } else { 40.0 };
        let foreground = self.look.palette.foreground;
        let ink = if accent {
            self.look.on_accent
        } else {
            foreground
        };
        let button = div()
            .id(id)
            .role(gpui::Role::Button)
            .aria_label(say(hint))
            .flex_none()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(glyph, px(size * 0.6), solid(ink)));
        let button = match (accent, self.wide) {
            (true, _) => button.bg(solid(self.look.accent)),
            (false, true) => button.bg(control_fill(&self.look)),
            (false, false) => button
                .bg(color(self.look.palette.glass, self.look.chip_alpha()))
                .border(px(floating::EDGE_WIDTH))
                .border_color(color(foreground, self.look.palette.edge_alpha)),
        };
        let button = if self.wide {
            button.rounded(control_radius(&self.look))
        } else {
            button.rounded_full()
        };
        if !live {
            return button.opacity(0.4).into_any_element();
        }
        let (background, text) = (solid(self.look.palette.raised), solid(foreground));
        self.tappable(button, cx, act)
            .tooltip(move |_, cx| {
                cx.new(|_| Hint {
                    said: hint,
                    background,
                    text,
                })
                .into()
            })
            .into_any_element()
    }

    /// A chip on the frost that says how things stand: nothing found,
    /// still waiting.
    pub(super) fn notice_row(&self, label: impl Into<SharedString>) -> Div {
        div()
            .pt(px(6.))
            .flex()
            .child(chip(&self.look).child(say(label)))
    }

    /// An island of rows under its label, the label inside it in accent
    /// text.
    pub(super) fn labelled(&self, label: impl Into<SharedString>) -> Div {
        island(&self.look).child(self.caption(label).px(px(ROW_PAD)).pt(px(12.)).pb(px(2.)))
    }

    /// A label inside an island, in accent text: what the rows under it
    /// are.
    pub(super) fn caption(&self, label: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(self.look.accent_text))
            .child(say(label).to_uppercase())
    }

    /// A panel of rows inside a card, set in from the card's edges and
    /// rounded to sit in its corners.
    pub(super) fn panel(&self) -> Div {
        div()
            .rounded(px(nested_radius(self.look.group_radius(), CARD_INSET)))
            .bg(color(self.look.palette.foreground, PANEL_FILL))
    }

    /// The corner of a first or last row, so its press fills the corner of
    /// what holds it: an island, or a panel inside a card.
    pub(super) fn row_radius(&self) -> gpui::Pixels {
        let island = self.look.group_radius();
        px(if self.nested.get() {
            nested_radius(island, CARD_INSET)
        } else {
            island
        })
    }

    /// The hairline between two rows, from `indent` in.
    pub(super) fn separator(&self, indent: f32) -> Div {
        div().h(px(1.)).ml(px(indent)).bg(color(
            self.look.palette.separator,
            self.look.palette.separator_alpha,
        ))
    }

    /// An icon on a square of the control fill, `size`.
    pub(super) fn tile(&self, glyph: Icon, size: f32) -> Div {
        div()
            .flex_none()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .child(icon(
                glyph,
                px(size * 0.58),
                solid(self.look.palette.foreground),
            ))
    }

    /// A title over the line that says more, in the secondary colour.
    pub(super) fn text_column(
        &self,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
    ) -> Div {
        self.text_lines(title, detail, false)
    }

    /// A row's title and the line under it; each cut to one line with an
    /// ellipsis when `one_line`, else wrapped.
    fn text_lines(
        &self,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
        one_line: bool,
    ) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            // Cut to a line it needs a box of its own; wrapped, the text
            // stands in the column itself, a box fewer on every row.
            .child(if one_line {
                div().truncate().child(say(title)).into_any_element()
            } else {
                say(title).into_any_element()
            })
            .children(detail.map(|detail| {
                div()
                    .when(one_line, |line| line.truncate())
                    .text_size(px(14.))
                    .text_color(solid(self.look.palette.secondary))
                    .child(say(detail))
            }))
    }

    /// A row that leads to a page: its tile, its title and, with `detail`,
    /// what it holds. Rounded as its island is where it is the island's
    /// `first` or `last`.
    pub(super) fn nav_row(
        &self,
        id: impl Into<gpui::ElementId>,
        glyph: Icon,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
        (first, last): (bool, bool),
    ) -> Stateful<Div> {
        let radius = px(self.look.group_radius());
        // Beside a page a row is as high as its tile and the tile's inset
        // twice, so the tile is as far from the row's top as from its side
        // whether one line stands beside it or two.
        let side = self.nav_side();
        let (pad, least) = if self.wide {
            (4.0, self.home_tile() + 2.0 * side)
        } else {
            (12.0, 0.0)
        };
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .px(px(side))
            .py(px(pad))
            .min_h(px(least))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
            .child(self.tile(glyph, self.home_tile()))
            // Beside a page the list is narrow: a line is never wrapped.
            .child(self.text_lines(title, detail, self.wide))
    }

    /// From a list row's side to its tile: less beside a page, where a
    /// pointer picks it.
    pub(super) fn nav_side(&self) -> f32 {
        if self.wide { 12.0 } else { ROW_PAD }
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
            .h(px(TOUCH))
            .px(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .truncate()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(ink))
            .child(say(label));
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
        // Under the pointer a button goes half the way to its press.
        let under = match self.fill(fill) {
            Some(fill) => solid(lerp_rgb(fill, foreground, PRESSED_FILL / 2.)),
            None => color(foreground, (CONTROL_FILL + PRESSED_FILL) / 2.),
        };
        button
            .cursor_pointer()
            .when(self.wide, |button| button.hover(move |s| s.bg(under)))
            .active(move |s| s.bg(pressed))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                action(this, window, cx)
            }))
    }

    /// Buttons side by side, as wide as their row.
    pub(super) fn buttons(&self, buttons: impl IntoIterator<Item = Stateful<Div>>) -> Div {
        div().flex().gap(px(10.)).children(buttons)
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
        let radius = self.row_radius();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .px(px(ROW_PAD))
            .py(px(12.))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
            .children(glyph.map(|glyph| self.tile(glyph, TILE)))
            .child(self.text_column(title, detail))
    }

    /// `rows` in an island, a hairline between each two from `indent` in.
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
            .child(say(value))
    }

    /// The chevron at the end of a row that opens something.
    pub(super) fn chevron(&self) -> gpui::Svg {
        icon(Icon::Chevron, px(20.), solid(self.look.palette.secondary))
    }

    /// An on or off switch.
    pub(super) fn toggle(&self, on: bool) -> Div {
        const WIDTH: f32 = 45.;
        const KNOB: f32 = 20.;
        const PAD: f32 = 3.;
        let rest = control_fill(&self.look);
        let accent = solid(self.look.accent);
        let (off_knob, on_knob) = (self.look.palette.secondary, self.look.on_accent);
        // The knob slides and the accent fades in over the resting track.
        let toggle = self.eased("toggle", on, move |at| {
            div()
                .relative()
                .w(px(WIDTH))
                .h(px(KNOB + 2. * PAD))
                .rounded_full()
                .bg(rest)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded_full()
                        .bg(accent)
                        .opacity(at),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(PAD))
                        .left(px(PAD + (WIDTH - KNOB - 2. * PAD) * at))
                        .size(px(KNOB))
                        .rounded_full()
                        .bg(solid(lerp_rgb(off_knob, on_knob, at))),
                )
        });
        div().flex_none().child(toggle)
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
            self.hovered(self.tappable(element, cx, action))
        } else {
            element
        }
    }
}
