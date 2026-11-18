use denial_look_gpui::{island, say};
use gpui::{Context, Div, prelude::*};
use serde_json::{Value, json};

use super::{Layer, Settings};
use crate::{catalog::INPUT_DECK_CONTROLS, denial::put};

const ENABLED: &str = "inputDeck.enabled";
const OUTPUT: &str = "inputDeck.outputName";

impl Settings {
    pub(in crate::settings) fn input_deck_page(
        &self,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let native = self.output_modes.input_deck.as_ref();
        let preferred = self.kept(OUTPUT).and_then(Value::as_str).unwrap_or("");
        let selected = native.and_then(|status| status.selected_output(preferred));
        let enabled = self.kept(ENABLED).and_then(Value::as_bool).unwrap_or(false);
        let ready = self.denial.document.is_some();
        let live = layer.live() && ready;
        let detail = match (native, selected) {
            (None, _) => say("Waiting for Input Deck status"),
            (_, None) => say("No touch display is available"),
            (Some(status), Some(_)) if !status.available => say("Waiting for an external display"),
            (_, Some(name)) => name.to_owned().into(),
        };
        let choices = native
            .filter(|status| status.touch_outputs.len() > 1)
            .map(|status| status.touch_outputs.clone());
        let row = self
            .row(
                "input-deck-enable",
                None,
                "Enable Input Deck",
                Some(detail),
                (true, choices.is_none()),
            )
            .role(gpui::Role::Switch)
            .child(self.toggle(enabled));
        let selected = selected.map(str::to_owned);
        let remembered = selected.clone();
        let mut group = island(&self.look).child(self.live(
            row,
            live && (enabled || selected.is_some()),
            cx,
            move |this, _, cx| {
                this.change(
                    "Input Deck",
                    {
                        let remembered = remembered.clone();
                        move |document| {
                            put(document, ENABLED, json!(!enabled));
                            if !enabled && let Some(name) = &remembered {
                                put(document, OUTPUT, json!(name));
                            }
                        }
                    },
                    cx,
                );
            },
        ));
        if let Some(choices) = choices {
            let selected = selected.unwrap_or_default();
            let row = self.output_option(
                "input-deck-display",
                "Touch display",
                selected.clone().into(),
                live,
                cx,
                move |this, _, cx| {
                    let choices = choices
                        .iter()
                        .map(|name| (name.clone(), name.clone().into()))
                        .collect();
                    this.output_picker(
                        "Touch display",
                        choices,
                        selected.clone(),
                        cx,
                        |this, name, _, cx| {
                            if this
                                .output_modes
                                .input_deck
                                .as_ref()
                                .is_some_and(|status| status.touch_outputs.contains(name))
                            {
                                let name = name.clone();
                                this.change(
                                    "Touch display",
                                    move |document| put(document, OUTPUT, json!(name)),
                                    cx,
                                );
                            }
                        },
                    );
                },
            );
            group = group.child(self.separator(0.)).child(row);
        }
        let mut parts = vec![self.chip_row("Input Deck"), group];
        if ready {
            parts.extend(self.page(INPUT_DECK_CONTROLS, layer, cx));
        }
        parts
    }
}
