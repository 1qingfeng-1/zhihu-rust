//! Selectable paragraph text with per-run styling.
//!
//! A paragraph is one layout and one selection participant. Word-level colors,
//! inline code, links and math are expressed as [`TextRun`]s inside that single
//! layout so selection stays cheap on long articles.

use std::ops::Range;

use gpui_kit::base::{
    TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun,
};
use gpui_kit::{
    px, transparent_black, App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, Font,
    FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    IntoElement, PaintQuad, Pixels, Point, Rgba, SharedString, StyledText, TextLayout, TextRun,
    TextStyleRefinement, UnderlineStyle, Window,
};
use zhihu_content::Inline;

use crate::render::{pos_color, text_chunks, LinkClickHandler};
use crate::{MONO_FONT, UI_FONT};

/// Text plus the styles and link ranges needed to lay it out selectably.
pub struct RichTextContent {
    text: String,
    runs: Vec<TextRun>,
    links: Vec<(Range<usize>, String)>,
}

#[allow(clippy::too_many_arguments)]
fn push_run(
    content: &mut RichTextContent,
    value: &str,
    family: &str,
    color: Rgba,
    bold: bool,
    italic: bool,
    background: Option<Rgba>,
    underline: bool,
) -> Range<usize> {
    let start = content.text.len();
    content.text.push_str(value);
    let end = content.text.len();
    if start == end {
        return start..end;
    }
    content.runs.push(TextRun {
        len: end - start,
        font: Font {
            family: family.into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        },
        color: color.into(),
        background_color: background.map(Into::into),
        underline: underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(color.into()),
            wavy: false,
        }),
        strikethrough: None,
    });
    start..end
}

/// Builds a paragraph's text, styled runs and link ranges.
#[allow(clippy::too_many_arguments)]
pub fn build(
    inlines: &[Inline],
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    surface: Rgba,
    pos_colors: bool,
    dark: bool,
    first_line_indent: bool,
    bold_all: bool,
    highlight: Option<(&str, Rgba)>,
) -> RichTextContent {
    let mut content = RichTextContent {
        text: String::new(),
        runs: Vec::new(),
        links: Vec::new(),
    };
    if first_line_indent {
        push_run(
            &mut content,
            "\u{3000}\u{3000}",
            UI_FONT,
            fg,
            false,
            false,
            None,
            false,
        );
    }
    for inline in inlines {
        match inline {
            Inline::Text {
                text,
                bold,
                italic,
                code,
                link,
            } => {
                let bold = *bold || bold_all;
                let chunks: Vec<(String, crate::pos::WordClass)> =
                    if pos_colors && link.is_none() && !*code {
                        crate::pos::segments(text)
                            .into_iter()
                            .map(|part| (part.text, part.class))
                            .collect()
                    } else {
                        text_chunks(text)
                            .into_iter()
                            .map(|chunk| (chunk.to_string(), crate::pos::WordClass::Plain))
                            .collect()
                    };
                for (chunk, class) in chunks {
                    let color = if link.is_some() || *code {
                        accent
                    } else {
                        pos_color(class, dark).unwrap_or(fg)
                    };
                    let range = push_run(
                        &mut content,
                        &chunk,
                        if *code { MONO_FONT } else { UI_FONT },
                        color,
                        bold,
                        *italic,
                        code.then_some(surface),
                        link.is_some(),
                    );
                    if let Some(url) = link {
                        content.links.push((range, url.clone()));
                    }
                }
            }
            Inline::Math(value) => {
                if !value.is_empty() {
                    let label = format!(" {} ", zhihu_content::format_math(value));
                    push_run(
                        &mut content,
                        &label,
                        UI_FONT,
                        accent,
                        false,
                        true,
                        None,
                        false,
                    );
                }
            }
            Inline::Image { alt, .. } => {
                let label = if alt.is_empty() {
                    "[图片]".to_string()
                } else {
                    format!("[图片: {alt}]")
                };
                push_run(
                    &mut content,
                    &label,
                    UI_FONT,
                    muted,
                    false,
                    false,
                    None,
                    false,
                );
            }
            Inline::LineBreak => {
                push_run(&mut content, "\n", UI_FONT, fg, false, false, None, false);
            }
        }
    }
    if let Some((quote, color)) = highlight {
        for fragment in quote.split('\n') {
            let fragment = fragment.trim();
            if !fragment.is_empty() {
                apply_highlight(&mut content, fragment, color);
            }
        }
    }
    content
}

fn apply_highlight(content: &mut RichTextContent, needle: &str, color: Rgba) {
    let Some(start) = content.text.find(needle) else {
        return;
    };
    let end = start + needle.len();
    let mut runs = Vec::with_capacity(content.runs.len() + 2);
    let mut offset = 0usize;
    for run in content.runs.drain(..) {
        let run_end = offset + run.len;
        let from = offset.max(start);
        let to = run_end.min(end);
        if from >= to {
            runs.push(run);
        } else {
            if offset < from {
                let mut before = run.clone();
                before.len = from - offset;
                runs.push(before);
            }
            let mut middle = run.clone();
            middle.len = to - from;
            middle.background_color = Some(Hsla::from(color).alpha(0.35));
            runs.push(middle);
            if to < run_end {
                let mut after = run;
                after.len = run_end - to;
                runs.push(after);
            }
        }
        offset = run_end;
    }
    content.runs = runs;
}

/// A paragraph-sized selectable text block.
pub struct SelectableRichText {
    id: ElementId,
    text: SharedString,
    styled: StyledText,
    links: Vec<(Range<usize>, String)>,
    link_click: Option<LinkClickHandler>,
    document_order: u64,
    text_style: TextStyleRefinement,
}

impl SelectableRichText {
    pub fn new(id: impl Into<ElementId>, content: RichTextContent, document_order: u64) -> Self {
        let styled = StyledText::new(content.text.clone()).with_runs(content.runs);
        Self {
            id: id.into(),
            text: content.text.into(),
            styled,
            links: content.links,
            link_click: None,
            document_order,
            text_style: TextStyleRefinement::default(),
        }
    }

    pub fn text_style(mut self, style: TextStyleRefinement) -> Self {
        self.text_style = style;
        self
    }

    pub fn link_click(mut self, handler: Option<LinkClickHandler>) -> Self {
        self.link_click = handler;
        self
    }

    fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };
        for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
            window.paint_quad(PaintQuad {
                bounds,
                background: color.into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }
    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}

struct RichSelectionState {
    handle: TextSelectionHandle,
    _refresh: gpui_kit::Subscription,
}

impl IntoElement for SelectableRichText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableRichText {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, Self::RequestLayoutState) {
        let handle = window.with_element_state(
            global_id.expect("SelectableRichText must have a stable element id"),
            |retained: Option<RichSelectionState>, window| {
                let state = retained.unwrap_or_else(|| {
                    let handle = TextSelectionHandle::new(self.text.clone(), cx);
                    let refresh = handle.refresh_window_on_change(window, cx);
                    RichSelectionState {
                        handle,
                        _refresh: refresh,
                    }
                });
                (state.handle.clone(), state)
            },
        );
        let style = self.text_style.clone();
        let (layout_id, ()) = window.with_text_style(Some(style), |window| {
            self.styled
                .request_layout(global_id, inspector_id, window, cx)
        });
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        handle.register(
            TextSelectionRegistration::new(hitbox.clone(), bounds)
                .with_document_order(self.document_order)
                .with_text_bounds(vec![bounds]),
            window,
            cx,
        );
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled.layout().clone();
        if let Some(handler) = self.link_click.clone() {
            let links = self.links.clone();
            let hitbox = hitbox.clone();
            let link_layout = layout.clone();
            window.on_mouse_event(move |event: &gpui_kit::MouseUpEvent, phase, window, cx| {
                if !phase.bubble() || !hitbox.is_hovered(window) {
                    return;
                }
                if TextSelection::has_selection(window, cx) {
                    return;
                }
                let Ok(index) = link_layout.index_for_position(event.position) else {
                    return;
                };
                if let Some((_, url)) = links.iter().find(|(range, _)| range.contains(&index)) {
                    handler(url.clone(), window, cx);
                }
            });
        }
        let before = TextSelection::selected_text(window, cx);
        let projection = handle.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                    .with_document_order(self.document_order),
            ],
            cx,
        );
        if before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        let color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        self.styled.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
        for range in projection.ranges().iter().flatten().cloned() {
            Self::paint_selection(&layout, range, color, window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    fn paragraph(text: &str) -> Vec<Inline> {
        vec![Inline::Text {
            text: text.to_string(),
            bold: false,
            italic: false,
            code: false,
            link: None,
        }]
    }

    fn build_with_quote(text: &str, quote: Option<&str>) -> RichTextContent {
        build(
            &paragraph(text),
            rgb(0x111111),
            rgb(0x222222),
            rgb(0x333333),
            rgb(0x444444),
            false,
            false,
            false,
            false,
            quote.map(|quote| (quote, rgb(0xff0000))),
        )
    }

    #[test]
    fn quote_highlight_marks_matching_runs() {
        let content = build_with_quote("第一句。第二句。", Some("第二句"));
        assert!(content
            .runs
            .iter()
            .any(|run| run.background_color.is_some()));
        assert_eq!(
            content.runs.iter().map(|run| run.len).sum::<usize>(),
            content.text.len()
        );
    }

    #[test]
    fn missing_quote_leaves_runs_plain() {
        let content = build_with_quote("第一句。第二句。", Some("不存在"));
        assert!(content
            .runs
            .iter()
            .all(|run| run.background_color.is_none()));
    }
}
