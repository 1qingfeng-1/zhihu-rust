//! Zhihu emoji/sticker lookup and inline text splitting.
//!
//! The sticker catalog is derived from Zhihu++ (AGPL-3.0). Images are embedded
//! into the binary and materialized into the app data directory on first use so
//! GPUI can load them through its normal file-image path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::emoji_data::EMOJIS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmojiPiece {
    Text(String),
    Emoji {
        placeholder: &'static str,
        path: PathBuf,
    },
}

fn lookup(placeholder: &str) -> Option<(&'static str, &'static [u8])> {
    EMOJIS.iter().find(|(key, _)| *key == placeholder).copied()
}

fn emoji_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = super::data_dir().join("emojis");
        fs::create_dir_all(&dir).ok()?;
        Some(dir)
    })
    .as_deref()
}

pub fn emoji_path(placeholder: &str) -> Option<PathBuf> {
    let (_, bytes) = lookup(placeholder)?;
    let dir = emoji_dir()?;
    let filename = format!(
        "{}.png",
        placeholder
            .trim_matches(['[', ']'])
            .replace(['/', '\\', ':'], "_")
    );
    let path = dir.join(filename);
    if !path.exists() {
        fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// Split a plain-text run into ordinary text and known Zhihu emoji tokens.
/// Unknown bracket tokens remain ordinary text.
pub fn pieces(text: &str) -> Vec<EmojiPiece> {
    let mut result = Vec::new();
    let mut plain_start = 0usize;
    let mut cursor = 0usize;
    while let Some(open_offset) = text[cursor..].find('[') {
        let open = cursor + open_offset;
        let Some(close_offset) = text[open..].find(']') else {
            break;
        };
        let close = open + close_offset + 1;
        let placeholder = &text[open..close];
        let Some((known, _)) = lookup(placeholder) else {
            cursor = open + 1;
            continue;
        };
        if plain_start < open {
            result.push(EmojiPiece::Text(text[plain_start..open].to_string()));
        }
        if let Some(path) = emoji_path(known) {
            result.push(EmojiPiece::Emoji {
                placeholder: known,
                path,
            });
        } else {
            result.push(EmojiPiece::Text(known.to_string()));
        }
        plain_start = close;
        cursor = close;
    }
    if plain_start < text.len() {
        result.push(EmojiPiece::Text(text[plain_start..].to_string()));
    }
    if result.is_empty() && !text.is_empty() {
        result.push(EmojiPiece::Text(text.to_string()));
    }
    result
}

pub fn has_emoji(text: &str) -> bool {
    matches!(pieces(text).as_slice(), [EmojiPiece::Emoji { .. }, ..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_known_and_unknown_tokens() {
        let items = pieces("你好[惊喜]世界[不存在]");
        assert!(matches!(&items[0], EmojiPiece::Text(text) if text == "你好"));
        assert!(
            matches!(&items[1], EmojiPiece::Emoji { placeholder, .. } if *placeholder == "[惊喜]")
        );
        assert!(matches!(&items[2], EmojiPiece::Text(text) if text == "世界[不存在]"));
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(
            pieces("普通文本"),
            vec![EmojiPiece::Text("普通文本".into())]
        );
        assert!(!has_emoji("普通文本"));
    }
}
