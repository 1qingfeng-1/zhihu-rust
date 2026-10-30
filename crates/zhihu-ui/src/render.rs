//! Rich-text block rendering for answer / article bodies.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::base::SelectableText;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, font, img, px, AnyElement, App, Font, FontStyle, FontWeight, Hsla, RenderImage, Rgba,
    SharedString, StyledText, TextRun, TextStyleRefinement, UnderlineStyle, Window,
};
use zhihu_content::{Block, Inline};

use crate::selectable;
use crate::typography::Typography;
use crate::{MONO_FONT, UI_FONT};

/// Callback invoked when the reader taps an inline video.
pub type VideoClickHandler = Arc<dyn Fn(String, &mut Window, &mut App) + 'static>;

/// Callback invoked when the reader taps a loaded image.
pub type ImageClickHandler = Arc<dyn Fn(PathBuf, &mut Window, &mut App) + 'static>;

/// Callback used by code/formula copy buttons.
pub type CopyHandler = Arc<dyn Fn(String, &mut Window, &mut App) + 'static>;

/// Callback used by inline hyperlinks.
pub(crate) type LinkClickHandler = Arc<dyn Fn(String, &mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, Debug, Default)]
pub struct ReadingEnhancements {
    pub first_line_indent: bool,
    pub pos_colors: bool,
    pub dark: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn render_blocks(
    blocks: &[Block],
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    surface: Rgba,
    border: Rgba,
    typography: &Typography,
    images: &HashMap<String, PathBuf>,
    image_errors: &HashSet<String>,
    video_click: Option<&VideoClickHandler>,
    image_click: Option<&ImageClickHandler>,
    copy: Option<&CopyHandler>,
    link_click: Option<&LinkClickHandler>,
    selectable: bool,
    highlight: Option<(&str, Rgba)>,
    enhancements: ReadingEnhancements,
) -> Vec<AnyElement> {
    let order = std::cell::Cell::new(0u64);
    render_blocks_with_order(
        blocks,
        fg,
        muted,
        accent,
        surface,
        border,
        typography,
        images,
        image_errors,
        video_click,
        image_click,
        copy,
        link_click,
        selectable,
        &order,
        highlight,
        enhancements,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_blocks_with_order(
    blocks: &[Block],
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    surface: Rgba,
    border: Rgba,
    typography: &Typography,
    images: &HashMap<String, PathBuf>,
    image_errors: &HashSet<String>,
    video_click: Option<&VideoClickHandler>,
    image_click: Option<&ImageClickHandler>,
    copy: Option<&CopyHandler>,
    link_click: Option<&LinkClickHandler>,
    selectable: bool,
    order: &std::cell::Cell<u64>,
    highlight: Option<(&str, Rgba)>,
    enhancements: ReadingEnhancements,
) -> Vec<AnyElement> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(inlines) => {
                if let Some(tex) = only_math(inlines) {
                    if let Some(formula) = render_formula(tex, typography.body_size * 1.15, fg) {
                        let mut row = div()
                            .w_full()
                            .py(px(typography.block_gap * 0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(
                                img(formula.image)
                                    .w(px(formula.width))
                                    .h(px(formula.height)),
                            );
                        if let Some(copy) = copy {
                            let handler = copy.clone();
                            let tex = tex.to_string();
                            row = row.child(
                                div()
                                    .id(SharedString::from(format!("formula-copy-{tex}")))
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .text_xs()
                                    .text_color(accent)
                                    .cursor_pointer()
                                    .on_click(move |_, window, cx| {
                                        handler(format!("$ {tex} $"), window, cx)
                                    })
                                    .child("复制公式"),
                            );
                        }
                        return row.into_any_element();
                    }
                }
                let has_emoji = inlines.iter().any(|inline| match inline {
                    Inline::Text { text, .. } => crate::emoji::has_emoji(text),
                    _ => false,
                });
                let rich = selectable && !has_emoji;
                let needs_flow = enhancements.first_line_indent
                    || enhancements.pos_colors
                    || inlines.iter().any(|inline| match inline {
                        Inline::Text { text, link, .. } => {
                            link.is_some() || crate::emoji::has_emoji(text)
                        }
                        _ => false,
                    });
                div()
                    .w_full()
                    .text_size(px(typography.body_size))
                    .line_height(px(typography.line_height))
                    .child(if rich {
                        let index = order.get();
                        order.set(index + 1);
                        let content = selectable::build(
                            inlines,
                            fg,
                            muted,
                            accent,
                            surface,
                            enhancements.pos_colors,
                            enhancements.dark,
                            enhancements.first_line_indent,
                            false,
                            highlight,
                        );
                        selectable::SelectableRichText::new(
                            SharedString::from(format!("rich-{index}")),
                            content,
                            index,
                        )
                        .text_style(selectable_text_style(
                            fg,
                            typography.body_size,
                            typography.line_height,
                            false,
                            false,
                            false,
                            false,
                        ))
                        .link_click(link_click.cloned())
                        .into_any_element()
                    } else if needs_flow {
                        inline_flow(
                            inlines,
                            fg,
                            muted,
                            accent,
                            surface,
                            typography,
                            link_click,
                            selectable,
                            order,
                            enhancements.first_line_indent,
                            enhancements,
                        )
                    } else {
                        styled_inlines(
                            inlines,
                            fg,
                            muted,
                            accent,
                            border,
                            false,
                            false,
                            typography.link_underline,
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            }
            Block::Heading { level, content } => {
                let size = typography.heading_size(*level);
                div()
                    .w_full()
                    .mt(px(typography.block_gap * 0.5))
                    .mb(px(typography.block_gap * 0.15))
                    .text_size(px(size))
                    .line_height(px(typography.heading_line_height(*level)))
                    .child(if selectable {
                        let index = order.get();
                        order.set(index + 1);
                        let content = selectable::build(
                            content,
                            fg,
                            muted,
                            accent,
                            surface,
                            enhancements.pos_colors,
                            enhancements.dark,
                            false,
                            true,
                            highlight,
                        );
                        selectable::SelectableRichText::new(
                            SharedString::from(format!("rich-{index}")),
                            content,
                            index,
                        )
                        .text_style(selectable_text_style(
                            fg,
                            size,
                            typography.heading_line_height(*level),
                            false,
                            true,
                            false,
                            false,
                        ))
                        .link_click(link_click.cloned())
                        .into_any_element()
                    } else {
                        styled_inlines(
                            content,
                            fg,
                            muted,
                            accent,
                            border,
                            true,
                            false,
                            typography.link_underline,
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            }
            Block::Quote(inner) => {
                let children = render_blocks_with_order(
                    inner,
                    muted,
                    muted,
                    accent,
                    surface,
                    border,
                    typography,
                    images,
                    image_errors,
                    video_click,
                    image_click,
                    copy,
                    link_click,
                    selectable,
                    order,
                    highlight,
                    enhancements,
                );
                div()
                    .w_full()
                    .pl(px(typography.quote_padding))
                    .pr(px(typography.quote_padding))
                    .py(px(typography.quote_padding * 0.6))
                    .rounded(px(typography.quote_radius))
                    .bg(surface)
                    .border_l_3()
                    .border_color(accent)
                    .flex()
                    .flex_col()
                    .gap(px(typography.block_gap * 0.6))
                    .children(children)
                    .into_any_element()
            }
            Block::List { ordered, items } => {
                let mut column = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(typography.list_item_gap));
                for (index, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}.", index + 1)
                    } else {
                        "•".to_string()
                    };
                    column = column.child(
                        div()
                            .w_full()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(typography.list_indent))
                                    .text_size(px(typography.body_size))
                                    .line_height(px(typography.line_height))
                                    .text_color(muted)
                                    .child(SharedString::from(marker)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(typography.body_size))
                                    .line_height(px(typography.line_height))
                                    .child({
                                        let has_emoji = item.iter().any(|inline| match inline {
                                            Inline::Text { text, .. } => {
                                                crate::emoji::has_emoji(text)
                                            }
                                            _ => false,
                                        });
                                        let needs_flow = has_emoji
                                            || enhancements.pos_colors
                                            || item.iter().any(|inline| match inline {
                                                Inline::Text { text, link, .. } => {
                                                    link.is_some() || crate::emoji::has_emoji(text)
                                                }
                                                _ => false,
                                            });
                                        if selectable && !has_emoji {
                                            let index = order.get();
                                            order.set(index + 1);
                                            let content = selectable::build(
                                                item,
                                                fg,
                                                muted,
                                                accent,
                                                surface,
                                                enhancements.pos_colors,
                                                enhancements.dark,
                                                false,
                                                false,
                                                highlight,
                                            );
                                            selectable::SelectableRichText::new(
                                                SharedString::from(format!("rich-{index}")),
                                                content,
                                                index,
                                            )
                                            .text_style(selectable_text_style(
                                                fg,
                                                typography.body_size,
                                                typography.line_height,
                                                false,
                                                false,
                                                false,
                                                false,
                                            ))
                                            .link_click(link_click.cloned())
                                            .into_any_element()
                                        } else if needs_flow {
                                            inline_flow(
                                                item,
                                                fg,
                                                muted,
                                                accent,
                                                surface,
                                                typography,
                                                link_click,
                                                selectable,
                                                order,
                                                false,
                                                enhancements,
                                            )
                                        } else {
                                            styled_inlines(
                                                item,
                                                fg,
                                                muted,
                                                accent,
                                                border,
                                                false,
                                                false,
                                                typography.link_underline,
                                            )
                                            .into_any_element()
                                        }
                                    }),
                            ),
                    );
                }
                column.into_any_element()
            }
            Block::Code { text, language } => {
                let mut header = div().flex().items_center().justify_between().w_full();
                if let Some(language) = language {
                    header = header.child(
                        div()
                            .text_size(px(typography.caption_size))
                            .text_color(muted)
                            .child(SharedString::from(language.clone())),
                    );
                } else {
                    header = header.child(div());
                }
                if let Some(copy) = copy {
                    let handler = copy.clone();
                    let code = text.clone();
                    header = header.child(
                        div()
                            .id(SharedString::from(format!("code-copy-{:p}", text.as_ptr())))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(border)
                            .text_size(px(typography.caption_size))
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| handler(code.clone(), window, cx))
                            .child("复制代码"),
                    );
                }
                div()
                    .id(SharedString::from(format!("code-{:p}", text.as_ptr())))
                    .w_full()
                    .rounded(px(typography.image_radius))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .p(px(typography.code_padding))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(header)
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(typography.code_size))
                            .line_height(px(typography.code_line_height))
                            .child(SharedString::from(text.clone())),
                    )
                    .into_any_element()
            }
            Block::Image { src, alt } => {
                let body: AnyElement = if let Some(path) = images.get(src) {
                    let picture = div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            img(path.clone())
                                .max_w_full()
                                .max_h(px(320.))
                                .object_fit(gpui_kit::ObjectFit::Contain),
                        )
                        .into_any_element();
                    if let Some(click) = image_click {
                        let click = click.clone();
                        let path = path.clone();
                        div()
                            .id(SharedString::from(format!("image-{src}")))
                            .w_full()
                            .cursor_pointer()
                            .on_click(move |_, window, cx| click(path.clone(), window, cx))
                            .child(picture)
                            .into_any_element()
                    } else {
                        picture
                    }
                } else {
                    let label = if image_errors.contains(src) {
                        format!("[图片加载失败] {alt}")
                    } else {
                        format!("[图片加载中] {alt}")
                    };
                    div()
                        .h(px(100.))
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(muted)
                        .child(SharedString::from(label))
                        .into_any_element()
                };
                div()
                    .w_full()
                    .min_h(px(120.))
                    .rounded(px(typography.image_radius))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(if alt.is_empty() {
                        div()
                    } else {
                        div()
                            .px(px(typography.code_padding))
                            .py_2()
                            .text_size(px(typography.caption_size))
                            .text_color(muted)
                            .text_center()
                            .child(SharedString::from(alt.clone()))
                    })
                    .into_any_element()
            }
            Block::Video { src, poster } => {
                let mut card = div()
                    .id(SharedString::from(format!("video-{src}")))
                    .w_full()
                    .rounded(px(typography.image_radius))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .cursor_pointer();
                if let Some(path) = images.get(poster) {
                    card = card.child(img(path.clone()).w_full().max_h(px(260.)));
                }
                card = card.child(
                    div()
                        .w_full()
                        .py_3()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(typography.body_size * 1.2))
                                .text_color(accent)
                                .child("▶"),
                        )
                        .child(
                            div()
                                .text_size(px(typography.body_size * 0.9))
                                .text_color(fg)
                                .child("播放视频"),
                        ),
                );
                if let Some(click) = video_click {
                    let click = click.clone();
                    let url = src.clone();
                    card = card.on_click(move |_, window, cx| click(url.clone(), window, cx));
                }
                card.into_any_element()
            }
            Block::Table(rows) => {
                let text = rows
                    .iter()
                    .map(|row| row.join("    "))
                    .collect::<Vec<_>>()
                    .join("\n");
                div()
                    .w_full()
                    .rounded(px(typography.image_radius))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .p(px(typography.code_padding))
                    .font_family(MONO_FONT)
                    .text_size(px(typography.code_size))
                    .line_height(px(typography.code_line_height))
                    .child(SharedString::from(text))
                    .into_any_element()
            }
            Block::Hr => div()
                .w_full()
                .h(px(1.))
                .bg(border)
                .my(px(typography.block_gap))
                .into_any_element(),
        })
        .collect()
}

struct FormulaImage {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
}

impl Clone for FormulaImage {
    fn clone(&self) -> Self {
        Self {
            image: self.image.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

type FormulaKey = (String, u32, u32, u32, u32, u32);

thread_local! {
    static FORMULA_CACHE: RefCell<HashMap<FormulaKey, Option<FormulaImage>>> =
        RefCell::new(HashMap::new());
}

fn only_math(inlines: &[Inline]) -> Option<&str> {
    let mut tex = None;
    for inline in inlines {
        match inline {
            Inline::Math(value) if tex.is_none() => tex = Some(value.as_str()),
            Inline::Text { text, .. } if text.trim().is_empty() => {}
            Inline::LineBreak => {}
            _ => return None,
        }
    }
    tex
}

fn render_formula(tex: &str, font_size: f32, color: Rgba) -> Option<FormulaImage> {
    let key = (
        tex.to_string(),
        font_size.to_bits(),
        color.r.to_bits(),
        color.g.to_bits(),
        color.b.to_bits(),
        color.a.to_bits(),
    );
    FORMULA_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(&key) {
            return entry.clone();
        }
        let rendered = ratex_gpui::render::render_latex(tex, font_size, 2.0, Hsla::from(color));
        let value = rendered.map(|rendered| FormulaImage {
            image: rendered.image,
            width: rendered.width,
            height: rendered.height,
        });
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(key, value.clone());
        value
    })
}

#[allow(clippy::too_many_arguments)]
fn inline_flow(
    inlines: &[Inline],
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    surface: Rgba,
    typography: &Typography,
    link_click: Option<&LinkClickHandler>,
    selectable: bool,
    order: &std::cell::Cell<u64>,
    first_line_indent: bool,
    enhancements: ReadingEnhancements,
) -> AnyElement {
    let mut row = div().w_full().flex().flex_wrap().items_center();
    if first_line_indent {
        row = row.child(
            div()
                .text_size(px(typography.body_size))
                .line_height(px(typography.line_height))
                .text_color(fg)
                .child("　　"),
        );
    }
    let mut sequence = 0usize;
    for inline in inlines {
        match inline {
            Inline::Text {
                text,
                bold,
                italic,
                code,
                link,
            } => {
                for piece in crate::emoji::pieces(text) {
                    match piece {
                        crate::emoji::EmojiPiece::Text(value) => {
                            let chunks: Vec<(String, crate::pos::WordClass)> =
                                if enhancements.pos_colors && link.is_none() && !*code {
                                    crate::pos::segments(&value)
                                        .into_iter()
                                        .map(|part| (part.text, part.class))
                                        .collect()
                                } else {
                                    text_chunks(&value)
                                        .into_iter()
                                        .map(|chunk| {
                                            (chunk.to_string(), crate::pos::WordClass::Plain)
                                        })
                                        .collect()
                                };
                            for (chunk, class) in chunks {
                                let color = if link.is_some() || *code {
                                    accent
                                } else {
                                    pos_color(class, enhancements.dark).unwrap_or(fg)
                                };
                                let mut span = if selectable {
                                    let index = order.get();
                                    order.set(index + 1);
                                    div().min_w_0().child(
                                        SelectableText::new(
                                            SharedString::from(format!("selection-run-{index}")),
                                            chunk,
                                        )
                                        .document_order(index)
                                        .text_style(
                                            selectable_text_style(
                                                color,
                                                typography.body_size,
                                                typography.line_height,
                                                *code,
                                                *bold,
                                                *italic,
                                                link.is_some() && typography.link_underline,
                                            ),
                                        ),
                                    )
                                } else {
                                    let mut span = div()
                                        .text_size(px(typography.body_size))
                                        .line_height(px(typography.line_height))
                                        .font_family(if *code { MONO_FONT } else { UI_FONT })
                                        .text_color(color)
                                        .child(SharedString::from(chunk));
                                    if *bold {
                                        span = span.font_weight(FontWeight::BOLD);
                                    }
                                    if *italic {
                                        span = span.italic();
                                    }
                                    span
                                };
                                if *code {
                                    span =
                                        span.px_1().rounded_sm().bg(surface).font_family(MONO_FONT);
                                }
                                if let (Some(url), Some(handler)) = (link, link_click) {
                                    let handler = handler.clone();
                                    let url = url.clone();
                                    row =
                                        row.child(
                                            span.id(SharedString::from(format!(
                                                "rich-link-{sequence}"
                                            )))
                                            .cursor_pointer()
                                            .underline()
                                            .on_click(move |_, window, cx| {
                                                handler(url.clone(), window, cx);
                                            }),
                                        );
                                    sequence += 1;
                                } else {
                                    row = row.child(span);
                                }
                            }
                        }
                        crate::emoji::EmojiPiece::Emoji { path, .. } => {
                            let mut icon =
                                img(path).size(px(typography.body_size * 1.2)).rounded_sm();
                            if let (Some(url), Some(handler)) = (link, link_click) {
                                let handler = handler.clone();
                                let url = url.clone();
                                icon = icon.cursor_pointer().on_click(move |_, window, cx| {
                                    handler(url.clone(), window, cx);
                                });
                            }
                            row = row.child(icon);
                        }
                    }
                }
            }
            Inline::Math(value) => {
                if !value.is_empty() {
                    row = row.child(
                        div()
                            .px_1()
                            .text_size(px(typography.body_size))
                            .line_height(px(typography.line_height))
                            .text_color(accent)
                            .italic()
                            .child(SharedString::from(format!(
                                " {} ",
                                zhihu_content::format_math(value)
                            ))),
                    );
                }
            }
            Inline::Image { alt, .. } => {
                let label = if alt.is_empty() {
                    "[图片]".to_string()
                } else {
                    format!("[图片: {alt}]")
                };
                row = row.child(
                    div()
                        .text_size(px(typography.body_size))
                        .text_color(muted)
                        .child(SharedString::from(label)),
                );
            }
            Inline::LineBreak => {
                row = row.child(div().w_full().h(px(0.)));
            }
        }
    }
    row.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn selectable_text_style(
    color: Rgba,
    size: f32,
    line_height: f32,
    code: bool,
    bold: bool,
    italic: bool,
    underline: bool,
) -> TextStyleRefinement {
    TextStyleRefinement {
        color: Some(color.into()),
        font_family: Some(if code { MONO_FONT } else { UI_FONT }.into()),
        font_size: Some(px(size).into()),
        line_height: Some(px(line_height).into()),
        font_weight: bold.then_some(FontWeight::BOLD),
        font_style: italic.then_some(FontStyle::Italic),
        underline: underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(color.into()),
            wavy: false,
        }),
        ..Default::default()
    }
}

pub(crate) fn pos_color(class: crate::pos::WordClass, dark: bool) -> Option<Rgba> {
    use crate::pos::WordClass;
    Some(match (class, dark) {
        (WordClass::Noun, true) => gpui_kit::rgb(0x8ab4d8),
        (WordClass::Noun, false) => gpui_kit::rgb(0x1f5f8b),
        (WordClass::Verb, true) => gpui_kit::rgb(0xd9ae72),
        (WordClass::Verb, false) => gpui_kit::rgb(0x8a5a13),
        (WordClass::Adjective, true) => gpui_kit::rgb(0x8fbf9f),
        (WordClass::Adjective, false) => gpui_kit::rgb(0x2f6f4f),
        (WordClass::Plain, _) => return None,
    })
}

pub(crate) fn text_chunks(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut count = 0usize;
    for (index, ch) in text.char_indices() {
        count += 1;
        let boundary = count >= 20
            || ch.is_whitespace()
            || matches!(
                ch,
                '，' | '。' | '！' | '？' | '；' | '：' | ',' | '.' | '!' | '?'
            );
        if boundary {
            let end = index + ch.len_utf8();
            if start < end {
                chunks.push(&text[start..end]);
            }
            start = end;
            count = 0;
        }
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

#[allow(clippy::too_many_arguments)]
fn styled_inlines(
    inlines: &[Inline],
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    code_bg: Rgba,
    bold_all: bool,
    italic_all: bool,
    link_underline: bool,
) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    let fg: Hsla = fg.into();
    let muted: Hsla = muted.into();
    let accent: Hsla = accent.into();
    let code_bg: Hsla = code_bg.into();

    for inline in inlines {
        match inline {
            Inline::Text {
                text: value,
                bold,
                italic,
                code,
                link,
            } => {
                let mut run_font = font(UI_FONT);
                if *bold || bold_all {
                    run_font = run_font.bold();
                }
                if *italic || italic_all {
                    run_font = run_font.italic();
                }
                if *code {
                    run_font = font(MONO_FONT);
                }
                let color = if link.is_some() || *code { accent } else { fg };
                let background = (*code && link.is_none()).then_some(code_bg);
                let underline = link.as_ref().and_then(|_| {
                    link_underline.then_some(UnderlineStyle {
                        color: Some(accent),
                        thickness: px(1.),
                        wavy: false,
                    })
                });
                append_run(
                    &mut text, &mut runs, value, run_font, color, background, underline,
                );
            }
            Inline::Math(value) => {
                if !value.is_empty() {
                    append_run(
                        &mut text,
                        &mut runs,
                        &format!(" {} ", zhihu_content::format_math(value)),
                        font(UI_FONT).italic(),
                        accent,
                        None,
                        None,
                    );
                }
            }
            Inline::Image { alt, .. } => {
                let label = if alt.is_empty() {
                    "[图片]".to_string()
                } else {
                    format!("[图片: {alt}]")
                };
                append_run(
                    &mut text,
                    &mut runs,
                    &label,
                    font(UI_FONT),
                    muted,
                    None,
                    None,
                );
            }
            Inline::LineBreak => {
                append_run(&mut text, &mut runs, "\n", font(UI_FONT), fg, None, None);
            }
        }
    }

    let styled = StyledText::new(SharedString::from(text));
    if runs.is_empty() {
        styled
    } else {
        styled.with_runs(runs)
    }
}

fn append_run(
    text: &mut String,
    runs: &mut Vec<TextRun>,
    value: &str,
    run_font: Font,
    color: Hsla,
    background_color: Option<Hsla>,
    underline: Option<UnderlineStyle>,
) {
    if value.is_empty() {
        return;
    }
    let len = value.len();
    text.push_str(value);
    runs.push(TextRun {
        len,
        font: run_font,
        color,
        background_color,
        underline,
        strikethrough: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_standalone_math_paragraphs() {
        let inlines = vec![
            Inline::Text {
                text: " ".to_string(),
                bold: false,
                italic: false,
                code: false,
                link: None,
            },
            Inline::Math(r"\frac{1}{2}".to_string()),
        ];
        assert_eq!(only_math(&inlines), Some(r"\frac{1}{2}"));
        let mixed = vec![
            Inline::Text {
                text: "速度".to_string(),
                bold: false,
                italic: false,
                code: false,
                link: None,
            },
            Inline::Math("v".to_string()),
        ];
        assert!(only_math(&mixed).is_none());
    }

    #[test]
    fn renders_latex_to_an_image() {
        let rendered = render_formula(
            r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}",
            20.0,
            gpui_kit::rgb(0xffffff),
        );
        let rendered = rendered.expect("formula should render");
        assert!(rendered.width > 0.0);
        assert!(rendered.height > 0.0);
    }
}
