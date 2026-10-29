//! Content pipeline: HTML → sanitized AST.
//!
//! The parser is intentionally a whitelist: only known text/image structures
//! become blocks; scripts, styles and unknown wrappers are unwrapped. Rendering
//! lives in `zhihu-ui` so this crate stays headless and testable.

use std::sync::OnceLock;

use scraper::{ElementRef, Html, Node, Selector};
pub use url::Url;

mod math;
pub use math::format_math;

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text {
        text: String,
        bold: bool,
        italic: bool,
        code: bool,
        link: Option<String>,
    },
    Image {
        src: String,
        alt: String,
    },
    Math(String),
    LineBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Quote(Vec<Block>),
    List {
        ordered: bool,
        items: Vec<Vec<Inline>>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Image {
        src: String,
        alt: String,
    },
    Video {
        src: String,
        poster: String,
    },
    Table(Vec<Vec<String>>),
    Hr,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

pub fn parse_html(html: &str) -> Document {
    let document = Html::parse_document(html);
    let root = document.root_element();
    let body = document.select(selector("body")).next().unwrap_or(root);
    let mut blocks = Vec::new();
    parse_block_children(body, &mut blocks);
    clean_blocks(&mut blocks);
    Document { blocks }
}

/// Collapse editor-generated whitespace noise:
///
/// - paragraphs made only of `&nbsp;` / `<br>` disappear;
/// - runs of 3+ `<br>` become a divider instead of a large blank gap;
/// - consecutive `<hr>` / dividers collapse to one;
/// - leading/trailing dividers are dropped.
fn clean_blocks(blocks: &mut Vec<Block>) {
    let mut cleaned = Vec::with_capacity(blocks.len());
    for block in blocks.drain(..) {
        match block {
            Block::Paragraph(inlines) => push_cleaned_paragraph(inlines, &mut cleaned),
            Block::Quote(mut inner) => {
                clean_blocks(&mut inner);
                if !inner.is_empty() {
                    cleaned.push(Block::Quote(inner));
                }
            }
            Block::List { ordered, items } => {
                let mut kept = Vec::new();
                for mut item in items {
                    item.retain(|inline| {
                        !matches!(inline, Inline::Text { text, .. } if text.trim().is_empty())
                    });
                    trim_inlines(&mut item);
                    if !item.is_empty() {
                        kept.push(item);
                    }
                }
                if !kept.is_empty() {
                    cleaned.push(Block::List {
                        ordered,
                        items: kept,
                    });
                }
            }
            other => cleaned.push(other),
        }
    }

    // Drop leading/trailing dividers and collapse duplicates.
    while matches!(cleaned.first(), Some(Block::Hr)) {
        cleaned.remove(0);
    }
    while matches!(cleaned.last(), Some(Block::Hr)) {
        cleaned.pop();
    }
    let mut deduped: Vec<Block> = Vec::with_capacity(cleaned.len());
    for block in cleaned {
        if matches!(block, Block::Hr) && matches!(deduped.last(), Some(Block::Hr)) {
            continue;
        }
        deduped.push(block);
    }
    *blocks = deduped;
}

fn push_cleaned_paragraph(inlines: Vec<Inline>, out: &mut Vec<Block>) {
    let mut current: Vec<Inline> = Vec::new();
    let mut breaks = 0usize;
    for inline in inlines {
        match inline {
            Inline::LineBreak => breaks += 1,
            Inline::Text { ref text, .. } if text.trim().is_empty() => {}
            other => {
                if breaks >= 3 {
                    if !current.is_empty() {
                        trim_inlines(&mut current);
                        if !current.is_empty() {
                            out.push(Block::Paragraph(std::mem::take(&mut current)));
                        }
                    }
                    out.push(Block::Hr);
                } else {
                    for _ in 0..breaks.min(2) {
                        current.push(Inline::LineBreak);
                    }
                }
                breaks = 0;
                current.push(other);
            }
        }
    }
    trim_inlines(&mut current);
    if !current.is_empty() {
        out.push(Block::Paragraph(current));
    }
}

/// Plain-text projection, useful for previews and search.
pub fn to_plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::Heading {
                content: inlines, ..
            } => {
                push_inline_text(inlines, &mut out);
                out.push('\n');
            }
            Block::Quote(blocks) => {
                out.push_str(&to_plain_text(blocks));
            }
            Block::List { items, .. } => {
                for item in items {
                    push_inline_text(item, &mut out);
                    out.push('\n');
                }
            }
            Block::Code { text, .. } => {
                out.push_str(text);
                out.push('\n');
            }
            Block::Image { alt, .. } => {
                out.push_str(alt);
                out.push('\n');
            }
            Block::Video { .. } => out.push_str("[视频]\n"),
            Block::Table(rows) => {
                for row in rows {
                    out.push_str(&row.join(" | "));
                    out.push('\n');
                }
            }
            Block::Hr => out.push_str("---\n"),
        }
    }
    out.trim_end().to_string()
}

/// Markdown projection for the "copy" actions. Rich structure (headings,
/// lists, quotes, code, images, math) is preserved; exact HTML formatting is
/// not.
pub fn to_markdown(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(inlines) => {
                push_inline_markdown(inlines, &mut out);
                out.push_str("\n\n");
            }
            Block::Heading { level, content } => {
                out.push_str(&"#".repeat((*level as usize).clamp(1, 6)));
                out.push(' ');
                push_inline_markdown(content, &mut out);
                out.push_str("\n\n");
            }
            Block::Quote(inner) => {
                let quoted = to_markdown(inner);
                for line in quoted.lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::List { ordered, items } => {
                for (index, item) in items.iter().enumerate() {
                    if *ordered {
                        out.push_str(&format!("{}. ", index + 1));
                    } else {
                        out.push_str("- ");
                    }
                    push_inline_markdown(item, &mut out);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::Code { language, text } => {
                out.push_str("```");
                if let Some(language) = language {
                    out.push_str(language);
                }
                out.push('\n');
                out.push_str(text);
                out.push_str("\n```\n\n");
            }
            Block::Image { src, alt } => {
                out.push_str(&format!("![{alt}]({src})\n\n"));
            }
            Block::Video { src, .. } => {
                out.push_str(&format!("[视频]({src})\n\n"));
            }
            Block::Table(rows) => {
                for (index, row) in rows.iter().enumerate() {
                    out.push_str("| ");
                    out.push_str(&row.join(" | "));
                    out.push_str(" |\n");
                    if index == 0 {
                        out.push('|');
                        for _ in row {
                            out.push_str(" --- |");
                        }
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
            Block::Hr => out.push_str("---\n\n"),
        }
    }
    out.trim_end().to_string()
}

fn push_inline_markdown(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text {
                text,
                bold,
                italic,
                code,
                link,
            } => {
                let mut rendered = text.clone();
                if *code {
                    rendered = format!("`{rendered}`");
                }
                if *bold {
                    rendered = format!("**{rendered}**");
                }
                if *italic {
                    rendered = format!("*{rendered}*");
                }
                if let Some(link) = link {
                    rendered = format!("[{rendered}]({link})");
                }
                out.push_str(&rendered);
            }
            Inline::Image { src, alt } => out.push_str(&format!("![{alt}]({src})")),
            Inline::Math(text) => {
                out.push('$');
                out.push_str(text);
                out.push('$');
            }
            Inline::LineBreak => out.push('\n'),
        }
    }
}

fn push_inline_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Math(text) => out.push_str(&format_math(text)),
            Inline::LineBreak => out.push('\n'),
        }
    }
}

#[derive(Clone, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    link: Option<String>,
}

fn parse_block_children(parent: ElementRef<'_>, out: &mut Vec<Block>) {
    let mut pending: Vec<Inline> = Vec::new();

    for child in parent.children() {
        match child.value() {
            Node::Text(text) => {
                push_text(
                    &mut pending,
                    &normalize_text(text.text.as_ref()),
                    &Style::default(),
                );
            }
            Node::Element(_) => {
                if let Some(element) = ElementRef::wrap(child) {
                    let name = element.value().name();
                    if is_block_tag(name) {
                        flush_paragraph(&mut pending, out);
                        parse_block_element(name, element, out);
                    } else {
                        parse_inline_element(name, element, &Style::default(), &mut pending);
                    }
                }
            }
            _ => {}
        }
    }
    flush_paragraph(&mut pending, out);
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "ul"
            | "ol"
            | "pre"
            | "figure"
            | "figcaption"
            | "video"
            | "table"
            | "hr"
    )
}

fn parse_block_element(name: &str, element: ElementRef<'_>, out: &mut Vec<Block>) {
    match name {
        "p" | "figcaption" => {
            let mut inlines = Vec::new();
            parse_inlines(element, &Style::default(), &mut inlines);
            trim_inlines(&mut inlines);
            if !inlines.is_empty() {
                out.push(Block::Paragraph(inlines));
            }
        }
        "div" | "section" | "article" | "header" | "footer" | "main" => {
            parse_block_children(element, out);
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name.as_bytes()[1] - b'0';
            let mut inlines = Vec::new();
            parse_inlines(element, &Style::default(), &mut inlines);
            trim_inlines(&mut inlines);
            if !inlines.is_empty() {
                out.push(Block::Heading {
                    level,
                    content: inlines,
                });
            }
        }
        "blockquote" => {
            let mut blocks = Vec::new();
            parse_block_children(element, &mut blocks);
            if !blocks.is_empty() {
                out.push(Block::Quote(blocks));
            }
        }
        "ul" | "ol" => {
            let ordered = name == "ol";
            let mut items = Vec::new();
            for child in element.children() {
                if let Some(li) = ElementRef::wrap(child) {
                    if li.value().name() == "li" {
                        let mut inlines = Vec::new();
                        parse_inlines(li, &Style::default(), &mut inlines);
                        if !inlines.is_empty() {
                            items.push(inlines);
                        }
                    }
                }
            }
            if !items.is_empty() {
                out.push(Block::List { ordered, items });
            }
        }
        "pre" => {
            let text = element.text().collect::<String>();
            let language = element
                .select(selector("code"))
                .next()
                .and_then(|code| code.value().attr("class"))
                .and_then(|class| {
                    class
                        .split_whitespace()
                        .find_map(|part| part.strip_prefix("language-"))
                })
                .map(str::to_string);
            if !text.trim().is_empty() {
                out.push(Block::Code {
                    language,
                    text: text.trim_end().to_string(),
                });
            }
        }
        "hr" => out.push(Block::Hr),
        "video" => {
            if let Some(block) = video_block(element) {
                out.push(block);
            }
        }
        "figure" => {
            let mut found = false;
            for video in element.select(selector("video")) {
                if let Some(block) = video_block(video) {
                    out.push(block);
                    found = true;
                }
            }
            for img in element.select(selector("img")) {
                if let Some(block) = image_block(img) {
                    out.push(block);
                    found = true;
                }
            }
            if let Some(caption) = element.select(selector("figcaption")).next() {
                let text = caption.text().collect::<String>();
                let text = normalize_text(&text).trim().to_string();
                if !text.is_empty() {
                    out.push(Block::Paragraph(vec![Inline::Text {
                        text,
                        bold: false,
                        italic: false,
                        code: false,
                        link: None,
                    }]));
                }
            }
            if !found {
                parse_block_children(element, out);
            }
        }
        "table" => {
            let mut rows = Vec::new();
            for row in element.select(selector("tr")) {
                let cells: Vec<String> = row
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|cell| matches!(cell.value().name(), "td" | "th"))
                    .map(|cell| {
                        normalize_text(&cell.text().collect::<String>())
                            .trim()
                            .to_string()
                    })
                    .collect();
                if !cells.is_empty() {
                    rows.push(cells);
                }
            }
            if !rows.is_empty() {
                out.push(Block::Table(rows));
            }
        }
        _ => parse_block_children(element, out),
    }
}

fn parse_inlines(element: ElementRef<'_>, style: &Style, out: &mut Vec<Inline>) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                push_text(out, &normalize_text(text.text.as_ref()), style);
            }
            Node::Element(_) => {
                if let Some(element) = ElementRef::wrap(child) {
                    parse_inline_element(element.value().name(), element, style, out);
                }
            }
            _ => {}
        }
    }
}

fn parse_inline_element(
    name: &str,
    element: scraper::ElementRef<'_>,
    style: &Style,
    out: &mut Vec<Inline>,
) {
    match name {
        "script" | "style" | "noscript" => {}
        "br" => out.push(Inline::LineBreak),
        "img" => {
            if let Some(inline) = image_inline(element) {
                out.push(inline);
            }
        }
        "b" | "strong" => {
            let mut next = style.clone();
            next.bold = true;
            parse_inlines(element, &next, out);
        }
        "i" | "em" => {
            let mut next = style.clone();
            next.italic = true;
            parse_inlines(element, &next, out);
        }
        "code" => {
            let mut next = style.clone();
            next.code = true;
            let text = element.text().collect::<String>();
            if !text.is_empty() {
                push_text(out, &text, &next);
            }
        }
        "a" => {
            let mut next = style.clone();
            next.link = element.value().attr("href").map(normalize_link);
            parse_inlines(element, &next, out);
        }
        "span" => {
            if let Some(tex) = element.value().attr("data-tex") {
                out.push(Inline::Math(tex.to_string()));
            } else {
                parse_inlines(element, style, out);
            }
        }
        "sup" | "sub" => {
            let text = element.text().collect::<String>();
            if !text.is_empty() {
                push_text(out, &text, style);
            }
        }
        _ => parse_inlines(element, style, out),
    }
}

fn image_inline(element: ElementRef<'_>) -> Option<Inline> {
    let src = image_src(element)?;
    let alt = element.value().attr("alt").unwrap_or_default().to_string();

    if src.contains("/equation?") {
        if let Ok(url) = Url::parse(&src) {
            if let Some((_, tex)) = url.query_pairs().find(|(key, _)| key == "tex") {
                return Some(Inline::Math(tex.to_string()));
            }
        }
        return Some(Inline::Math(alt));
    }

    Some(Inline::Image { src, alt })
}

fn image_block(element: ElementRef<'_>) -> Option<Block> {
    let src = image_src(element)?;
    if src.contains("/equation?") {
        return None;
    }
    Some(Block::Image {
        src,
        alt: element.value().attr("alt").unwrap_or_default().to_string(),
    })
}

fn video_block(element: ElementRef<'_>) -> Option<Block> {
    let src = element
        .value()
        .attr("src")
        .or_else(|| element.value().attr("data-src"))
        .or_else(|| element.value().attr("data-original"))
        .or_else(|| element.value().attr("data-url"))
        .map(str::trim)
        .filter(|src| !src.is_empty() && !src.starts_with("blob:"))?;
    let src = if src.starts_with("//") {
        format!("https:{src}")
    } else {
        src.to_string()
    };
    Some(Block::Video {
        src,
        poster: element
            .value()
            .attr("poster")
            .unwrap_or_default()
            .to_string(),
    })
}

fn image_src(element: ElementRef<'_>) -> Option<String> {
    element
        .value()
        .attr("data-original")
        .or_else(|| element.value().attr("src"))
        .filter(|src| !src.starts_with("data:"))
        .map(str::to_string)
}

fn normalize_link(href: &str) -> String {
    if let Ok(url) = Url::parse(href) {
        if url.host_str() == Some("link.zhihu.com") {
            if let Some((_, target)) = url.query_pairs().find(|(key, _)| key == "target") {
                return Url::parse(&target)
                    .map(|url| url.to_string())
                    .unwrap_or_else(|_| target.into_owned());
            }
        }
        return url.to_string();
    }
    if href.starts_with('/') {
        format!("https://www.zhihu.com{href}")
    } else {
        href.to_string()
    }
}

fn push_text(out: &mut Vec<Inline>, text: &str, style: &Style) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text {
        text: previous,
        bold,
        italic,
        code,
        link,
    }) = out.last_mut()
    {
        if *bold == style.bold
            && *italic == style.italic
            && *code == style.code
            && *link == style.link
        {
            previous.push_str(text);
            return;
        }
    }
    out.push(Inline::Text {
        text: text.to_string(),
        bold: style.bold,
        italic: style.italic,
        code: style.code,
        link: style.link.clone(),
    });
}

fn flush_paragraph(pending: &mut Vec<Inline>, out: &mut Vec<Block>) {
    trim_inlines(pending);
    if !pending.is_empty() {
        out.push(Block::Paragraph(std::mem::take(pending)));
    }
}

fn trim_inlines(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Text { text, .. }) = inlines.first_mut() {
        *text = text.trim_start().to_string();
    }
    if let Some(Inline::Text { text, .. }) = inlines.last_mut() {
        *text = text.trim_end().to_string();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text { text, .. } if text.is_empty()));
}

fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

fn selector(query: &'static str) -> &'static Selector {
    static CACHE: OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, &'static Selector>>,
    > = OnceLock::new();
    // Selectors for a handful of static strings; parse once per call site.
    // This small cache avoids reparsing on every block while staying `Send`.
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut cache = cache.lock().expect("selector cache poisoned");
    if let Some(selector) = cache.get(query) {
        return selector;
    }
    let selector = Box::leak(Box::new(Selector::parse(query).expect("valid selector")));
    cache.insert(query, selector);
    selector
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_and_blocks() {
        let html = r#"
            <h2>标题</h2>
            <p>这是<b>粗体</b>和<i>斜体</i>，还有<a href="https://link.zhihu.com/?target=https%3A%2F%2Fexample.com">链接</a>。</p>
            <p>公式 <img src="https://www.zhihu.com/equation?tex=E%3Dmc%5E2" alt="E=mc^2"/></p>
            <ul><li>第一项</li><li>第二项</li></ul>
            <blockquote><p>引用</p></blockquote>
            <pre><code class="language-rust">fn main() {}</code></pre>
            <hr/>
            <figure><img src="https://pic1.zhimg.com/x.jpg" alt="图"/><figcaption>说明</figcaption></figure>
        "#;
        let doc = parse_html(html);
        assert!(matches!(doc.blocks[0], Block::Heading { level: 2, .. }));
        let Block::Paragraph(inlines) = &doc.blocks[1] else {
            panic!("expected paragraph");
        };
        assert!(inlines.iter().any(|inline| matches!(
            inline,
            Inline::Text { text, bold: true, .. } if text == "粗体"
        )));
        assert!(inlines.iter().any(|inline| matches!(
            inline,
            Inline::Text { link: Some(link), .. } if link == "https://example.com/"
        )));
        assert!(
            matches!(doc.blocks[2], Block::Paragraph(ref i) if i.contains(&Inline::Math("E=mc^2".into())))
        );
        assert!(
            matches!(doc.blocks[3], Block::List { ordered: false, ref items } if items.len() == 2)
        );
        assert!(matches!(doc.blocks[4], Block::Quote(_)));
        assert!(
            matches!(doc.blocks[5], Block::Code { ref language, .. } if language.as_deref() == Some("rust"))
        );
        assert!(matches!(doc.blocks[6], Block::Hr));
        assert!(matches!(doc.blocks[7], Block::Image { ref alt, .. } if alt == "图"));
    }

    #[test]
    fn parses_video_blocks() {
        let doc = parse_html(
            r#"<figure><video src="//v.vzuu.com/x.mp4" poster="https://pic1.zhimg.com/p.jpg"></video></figure>
               <video data-src="https://v.vzuu.com/y.mp4"></video>"#,
        );
        assert!(matches!(
            doc.blocks[0],
            Block::Video { ref src, ref poster }
                if src == "https://v.vzuu.com/x.mp4"
                    && poster == "https://pic1.zhimg.com/p.jpg"
        ));
        assert!(matches!(
            doc.blocks[1],
            Block::Video { ref src, .. } if src == "https://v.vzuu.com/y.mp4"
        ));
        assert_eq!(to_plain_text(&doc.blocks), "[视频]\n[视频]");
    }

    #[test]
    fn drops_scripts_and_unwraps_unknown() {
        let doc = parse_html(r#"<div><script>alert(1)</script><span>hello</span> world</div>"#);
        let text = to_plain_text(&doc.blocks);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn plain_text_roundtrip() {
        let doc = parse_html("<p>你好，<b>世界</b>。</p>");
        assert_eq!(to_plain_text(&doc.blocks), "你好，世界。");
    }

    #[test]
    fn renders_markdown_for_copy() {
        let doc = parse_html(
            "<h2>标题</h2><p>你好，<b>世界</b>与<a href=\"https://example.com\">链接</a>。</p><ul><li>一</li><li>二</li></ul><pre><code class=\"language-rust\">fn main() {}</code></pre>",
        );
        let markdown = to_markdown(&doc.blocks);
        assert!(markdown.contains("## 标题"));
        assert!(markdown.contains("**世界**"));
        assert!(markdown.contains("[链接](https://example.com/)"));
        assert!(markdown.contains("- 一"));
        assert!(markdown.contains("```rust"));
    }

    #[test]
    fn cleans_editor_whitespace_noise() {
        let doc = parse_html(
            "<p>a<br><br><br><br>b</p><p>&nbsp; </p><p><br><br></p><p>c</p><hr><hr><p>d</p>",
        );
        assert_eq!(doc.blocks.len(), 6);
        assert!(matches!(doc.blocks[0], Block::Paragraph(ref i) if i == &[text("a")]));
        assert!(matches!(doc.blocks[1], Block::Hr));
        assert!(matches!(doc.blocks[2], Block::Paragraph(ref i) if i == &[text("b")]));
        assert!(matches!(doc.blocks[3], Block::Paragraph(ref i) if i == &[text("c")]));
        assert!(matches!(doc.blocks[4], Block::Hr));
        assert!(matches!(doc.blocks[5], Block::Paragraph(ref i) if i == &[text("d")]));

        let two = parse_html("<p>e<br><br>f</p>");
        assert!(matches!(
            two.blocks[0],
            Block::Paragraph(ref i)
                if i == &[text("e"), Inline::LineBreak, Inline::LineBreak, text("f")]
        ));

        let spaces = parse_html("<p>a&nbsp;&nbsp;&nbsp; b</p>");
        assert_eq!(to_plain_text(&spaces.blocks), "a b");
    }

    fn text(value: &str) -> Inline {
        Inline::Text {
            text: value.to_string(),
            bold: false,
            italic: false,
            code: false,
            link: None,
        }
    }
}
