//! Lightweight Chinese POS segmentation for reading emphasis.

use std::sync::OnceLock;

use jieba_rs::Jieba;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordClass {
    Noun,
    Verb,
    Adjective,
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordSegment {
    pub text: String,
    pub class: WordClass,
}

fn jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

pub fn warm_up() {
    let _ = jieba();
}

fn classify(tag: &str) -> WordClass {
    match tag.as_bytes().first().copied() {
        Some(b'n') => WordClass::Noun,
        Some(b'v') => WordClass::Verb,
        Some(b'a') => WordClass::Adjective,
        _ => WordClass::Plain,
    }
}

pub fn segments(text: &str) -> Vec<WordSegment> {
    let mut result: Vec<WordSegment> = Vec::new();
    for tagged in jieba().tag(text, true) {
        let class = classify(tagged.tag);
        if let Some(last) = result.last_mut() {
            if last.class == class {
                last.text.push_str(tagged.word);
                continue;
            }
        }
        result.push(WordSegment {
            text: tagged.word.to_string(),
            class,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_words_and_preserves_text() {
        let text = "我喜欢阅读优秀的书籍。";
        let segments = segments(text);
        assert_eq!(
            segments
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(segments.iter().any(|part| part.class != WordClass::Plain));
    }

    #[test]
    fn empty_text_has_no_segments() {
        assert!(segments("").is_empty());
    }
}
