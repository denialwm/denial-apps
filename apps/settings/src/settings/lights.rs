//! The Lights page: the device's own lights, each on or off in a colour,
//! and whether they go dark with the screen. Denial sets the lights from
//! what this page writes to its settings document.

use denial_look::words::tr;
use serde_json::{Value, json};

use denial_look_gpui::{Icon, island, solid};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{Layer, Settings};
use crate::lights::{self, COLORS};

/// A colour swatch's diameter: a fingertip's.
const SWATCH: f32 = 40.;

impl Settings {
    pub(super) fn lights_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        if self.lights.is_empty() {
            return vec![self.missing(
                Icon::Palette,
                "No lights",
                "This device has no lights of its own",
            )];
        }
        if self.denial.document.is_none() {
            return vec![self.missing(
                Icon::Palette,
                "Lights are not available",
                "Denial, which sets the lights, does not answer",
            )];
        }
        let live = layer.live();
        let lights = self.rows_island(self.lights.iter(), 0., |light, ends| {
            let (on, color) = self.light_choice(&light.name);
            let row = self
                .row(
                    SharedString::from(format!("light:{}", light.name)),
                    Some(Icon::Palette),
                    light.title.clone(),
                    None,
                    ends,
                )
                .child(self.toggle(on));
            let name = light.name.clone();
            let row = self.live(row, live, cx, move |this, _, cx| {
                this.set_light(name.clone(), !on, color, cx)
            });
            if !on {
                return row;
            }
            let swatches = COLORS.iter().enumerate().map(|(ix, &swatch)| {
                let chosen = swatch == color;
                let name = light.name.clone();
                let dot = div()
                    .id(SharedString::from(format!("light:{}:{ix}", light.name)))
                    .size(px(SWATCH))
                    .rounded_full()
                    .bg(solid(swatch))
                    .border_2()
                    .border_color(if chosen {
                        solid(self.look.palette.foreground)
                    } else {
                        solid(self.look.palette.separator).opacity(0.)
                    });
                self.live(dot, live, cx, move |this, _, cx| {
                    this.set_light(name.clone(), true, swatch, cx)
                })
            });
            div()
                .id(SharedString::from(format!("light-colors:{}", light.name)))
                .flex()
                .flex_col()
                .child(row)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(12.))
                        .px(px(16.))
                        .pb(px(14.))
                        .children(swatches),
                )
        });
        let off_with_screen = self.lights_off_with_screen();
        let row = self
            .row(
                "lights-off-with-screen",
                Some(Icon::Moon),
                tr("Turn off with the screen"),
                Some(tr("The lights go dark when the screen does, and come back with it.").into()),
                (true, true),
            )
            .child(self.toggle(off_with_screen));
        let row = self.live(row, live, cx, move |this, _, cx| {
            this.set_lights_off_with_screen(!off_with_screen, cx)
        });
        vec![lights, island(&self.look).child(row)]
    }

    /// Whether `name` is on, and its colour: off, in the first colour, until
    /// chosen.
    fn light_choice(&self, name: &str) -> (bool, u32) {
        let light = self
            .denial
            .document
            .as_ref()
            .map(|document| &document["lights"]["leds"][name]);
        let on = light
            .and_then(|light| light["on"].as_bool())
            .unwrap_or(false);
        let color = light
            .and_then(|light| light["color"].as_str())
            .and_then(lights::parse)
            .unwrap_or(COLORS[0]);
        (on, color)
    }

    fn lights_off_with_screen(&self) -> bool {
        self.kept("lights.offWithScreen")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    fn set_light(&mut self, name: String, on: bool, color: u32, cx: &mut Context<Self>) {
        let choice = json!({"on": on, "color": lights::hex(color)});
        self.edit_lights(cx, move |document| {
            document["lights"]["leds"][name.as_str()] = choice.clone();
        });
    }

    fn set_lights_off_with_screen(&mut self, off: bool, cx: &mut Context<Self>) {
        self.edit_lights(cx, move |document| {
            document["lights"]["offWithScreen"] = json!(off);
        });
    }

    /// Changes the lights in Denial's document, here at once and then in
    /// Denial, which sets them.
    fn edit_lights(&mut self, cx: &mut Context<Self>, edit: impl Fn(&mut Value) + Send + 'static) {
        if let Some(document) = self.denial.document.as_mut() {
            edit(document);
        }
        cx.notify();
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.edit(&edit).await {
                let _ = this.update(cx, |view, cx| view.note(error.0, cx));
            }
        })
        .detach();
    }
}
