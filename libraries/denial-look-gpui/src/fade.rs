//! The scroll fade. GPUI cannot blur, so content passing under a glass group
//! would show sharp text through the tint; instead each item of a scrolling
//! list fades as it nears the viewport's ends.
//!
//! `Faded` wraps a plain `Div` and, when it paints, sets the div's opacity
//! from where it is inside the clip its scrolling parent paints it in: a
//! `list` or an `overflow_y_scroll` div. Div computes its style at paint time,
//! so the opacity is the one for this frame's position, without a frame of
//! lag while a list flings.

use denial_look::mobile::scroll_fade;
use gpui::{
    App, Bounds, Div, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Styled, Window, accesskit,
};

pub struct Faded {
    inner: Div,
}

/// `item`, fading by its place in the scroll viewport around it. An opacity
/// already set on `item` multiplies with the fade.
pub fn faded(item: Div) -> Faded {
    Faded { inner: item }
}

impl IntoElement for Faded {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Everything but painting goes to the div as it is.
impl Element for Faded {
    type RequestLayoutState = <Div as Element>::RequestLayoutState;
    type PrepaintState = <Div as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node)
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = window.content_mask().bounds;
        let fade = scroll_fade(
            bounds.top().as_f32(),
            bounds.bottom().as_f32(),
            view.top().as_f32(),
            view.bottom().as_f32(),
        );
        let style = self.inner.style();
        style.opacity = Some(style.opacity.unwrap_or(1.0) * fade);
        self.inner.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        )
    }
}
