//! Parse an HTML file and print a block summary.
//!
//! ```bash
//! cargo run -p zhihu-content --example parse_fixture -- path/to/answer.html
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: parse_fixture <file.html>");
    let html = std::fs::read_to_string(path)?;
    let document = zhihu_content::parse_html(&html);
    println!("blocks: {}", document.blocks.len());
    for (index, block) in document.blocks.iter().enumerate().take(40) {
        let summary = match block {
            zhihu_content::Block::Paragraph(inlines) => {
                format!("paragraph({} inlines) {}", inlines.len(), preview(inlines))
            }
            zhihu_content::Block::Heading { level, content } => {
                format!("h{level} {}", preview(content))
            }
            zhihu_content::Block::Quote(blocks) => format!("quote({} blocks)", blocks.len()),
            zhihu_content::Block::List { ordered, items } => {
                format!("list(ordered={ordered}, {} items)", items.len())
            }
            zhihu_content::Block::Code { language, text } => {
                format!("code({language:?}, {} bytes)", text.len())
            }
            zhihu_content::Block::Image { src, alt } => format!("image({alt}) {src}"),
            zhihu_content::Block::Video { src, .. } => format!("video {src}"),
            zhihu_content::Block::Table(rows) => format!("table({} rows)", rows.len()),
            zhihu_content::Block::Hr => "hr".to_string(),
        };
        println!("{index:03} {summary}");
    }
    Ok(())
}

fn preview(inlines: &[zhihu_content::Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            zhihu_content::Inline::Text { text, .. } => out.push_str(text),
            zhihu_content::Inline::Image { alt, .. } => out.push_str(alt),
            zhihu_content::Inline::Math(text) => out.push_str(text),
            zhihu_content::Inline::LineBreak => out.push(' '),
        }
        if out.chars().count() > 60 {
            break;
        }
    }
    out.chars().take(60).collect()
}
