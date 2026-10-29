//! Optional on-device feed reranking.
//!
//! The server order stays the source of truth. `Hybrid` blends it with local
//! interest signals, while `Local` sorts purely by those signals. Everything
//! here is pure CPU and local: no reading behaviour leaves the device.
//!
//! Interest comes from explicit feedback records (opening an answer/article,
//! upvoting, following an author). Topic and author weights decay with a
//! 30-day half-life, and a greedy diversity pass discourages three items from
//! the same author or topic in a row.

use std::collections::HashMap;

use zhihu_types::FeedItem;

const TOPIC_WEIGHT_CAP: f32 = 5.0;
const AUTHOR_WEIGHT_CAP: f32 = 5.0;
const HALF_LIFE_DAYS: f32 = 30.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecommendMode {
    /// Keep the server order.
    #[default]
    Off,
    /// Blend server position with local interest.
    Hybrid,
    /// Sort by local interest only.
    Local,
}

impl RecommendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RecommendMode::Off => "off",
            RecommendMode::Hybrid => "hybrid",
            RecommendMode::Local => "local",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "hybrid" => RecommendMode::Hybrid,
            "local" => RecommendMode::Local,
            _ => RecommendMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RecommendMode::Off => "关闭",
            RecommendMode::Hybrid => "混合",
            RecommendMode::Local => "本地",
        }
    }
}

/// One explicitly recorded interaction, already attributed to topics/author.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackRecord {
    pub author_id: String,
    pub topics: Vec<String>,
    pub weight: f32,
    pub created_at: i64,
}

/// Aggregated, time-decayed interest in topics and authors.
#[derive(Clone, Debug, Default)]
pub struct InterestProfile {
    pub topics: HashMap<String, f32>,
    pub authors: HashMap<String, f32>,
    max_topic: f32,
    max_author: f32,
}

impl InterestProfile {
    pub fn from_feedback(records: &[FeedbackRecord], now: i64) -> Self {
        let mut profile = Self::default();
        for record in records {
            let age_days = (now - record.created_at).max(0) as f32 / 86_400.0;
            let decay = 0.5f32.powf(age_days / HALF_LIFE_DAYS);
            let weight = record.weight * decay;
            if weight == 0.0 {
                continue;
            }
            for topic in &record.topics {
                if topic.is_empty() {
                    continue;
                }
                let slot = profile.topics.entry(topic.clone()).or_insert(0.0);
                *slot = (*slot + weight).clamp(0.0, TOPIC_WEIGHT_CAP);
            }
            if !record.author_id.is_empty() {
                let slot = profile
                    .authors
                    .entry(record.author_id.clone())
                    .or_insert(0.0);
                *slot = (*slot + weight).clamp(0.0, AUTHOR_WEIGHT_CAP);
            }
        }
        profile.max_topic = profile.topics.values().copied().fold(0.0, f32::max);
        profile.max_author = profile.authors.values().copied().fold(0.0, f32::max);
        profile
    }

    pub fn is_empty(&self) -> bool {
        self.topics.is_empty() && self.authors.is_empty()
    }
}

#[derive(Clone, Debug)]
struct Scored {
    item: FeedItem,
    score: f32,
    position: usize,
}

/// Reorder `items` in place. `Off` and cold-start profiles keep the server
/// order untouched.
pub fn rerank(items: &mut Vec<FeedItem>, profile: &InterestProfile, mode: RecommendMode) {
    if mode == RecommendMode::Off || items.len() < 2 || profile.is_empty() {
        return;
    }
    let total = items.len();
    let mut scored: Vec<Scored> = items
        .drain(..)
        .enumerate()
        .map(|(position, item)| {
            let local = local_score(&item, profile, position, total);
            let server = 1.0 - position as f32 / (total - 1) as f32;
            let score = match mode {
                RecommendMode::Hybrid => 0.55 * local + 0.45 * server,
                RecommendMode::Local => local,
                RecommendMode::Off => server,
            };
            Scored {
                item,
                score,
                position,
            }
        })
        .collect();

    let mut output: Vec<FeedItem> = Vec::with_capacity(total);
    while !scored.is_empty() {
        let mut best_index = 0;
        let mut best_score = f32::NEG_INFINITY;
        for (index, candidate) in scored.iter().enumerate() {
            let adjusted = candidate.score - diversity_penalty(&candidate.item, &output);
            let better = adjusted > best_score + 1e-6
                || ((adjusted - best_score).abs() <= 1e-6
                    && candidate.position < scored[best_index].position);
            if better {
                best_index = index;
                best_score = adjusted;
            }
        }
        output.push(scored.remove(best_index).item);
    }
    *items = output;
}

pub fn title_tokens(title: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "什么", "怎么", "如何", "为什", "么样", "怎样", "哪些", "有哪", "一个", "我们", "你们",
        "他们", "可以", "是否", "真的", "不是", "是什", "知乎", "评价", "觉得", "the", "and",
        "for", "with", "that", "this", "from", "what", "how", "why",
    ];
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_word = |word: &mut String, tokens: &mut Vec<String>| {
        if word.chars().count() >= 3 && !STOP.contains(&word.as_str()) {
            tokens.push(format!("t:{word}"));
        }
        word.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, tokens: &mut Vec<String>| {
        for window in run.windows(2) {
            if window.iter().any(|ch| is_function_char(*ch)) {
                continue;
            }
            let token: String = window.iter().collect();
            if !STOP.contains(&token.as_str()) {
                tokens.push(format!("t:{token}"));
            }
        }
        run.clear();
    };

    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk_run, &mut tokens);
            word.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_word(&mut word, &mut tokens);
            cjk_run.push(ch);
        } else {
            flush_word(&mut word, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }
    flush_word(&mut word, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);

    let mut seen = std::collections::HashSet::new();
    tokens.retain(|token| seen.insert(token.clone()));
    tokens.truncate(12);
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// Function characters add noise to character-bigram features, e.g. "么几"
/// or "个朋". Bigrams touching one of these are skipped.
fn is_function_char(ch: char) -> bool {
    matches!(
        ch,
        '的' | '了'
            | '是'
            | '在'
            | '有'
            | '和'
            | '与'
            | '就'
            | '都'
            | '也'
            | '很'
            | '我'
            | '你'
            | '他'
            | '她'
            | '它'
            | '们'
            | '这'
            | '那'
            | '么'
            | '什'
            | '吗'
            | '呢'
            | '啊'
            | '吧'
            | '被'
            | '把'
            | '让'
            | '从'
            | '到'
            | '对'
            | '为'
            | '以'
            | '而'
            | '及'
            | '或'
            | '但'
            | '却'
            | '个'
            | '几'
            | '自'
            | '将'
            | '着'
            | '过'
            | '地'
            | '得'
            | '之'
            | '其'
            | '于'
            | '阿'
    )
}

fn local_score(item: &FeedItem, profile: &InterestProfile, position: usize, total: usize) -> f32 {
    let topic_sum: f32 = item
        .topics
        .iter()
        .map(|topic| profile.topics.get(topic).copied().unwrap_or(0.0))
        .sum();
    let title_sum: f32 = title_tokens(&item.title)
        .iter()
        .map(|token| profile.topics.get(token).copied().unwrap_or(0.0))
        .sum();
    let feature_sum = topic_sum + 0.5 * title_sum;
    let topic = feature_sum / (feature_sum + profile.max_topic.max(1.0));
    let author = if item.author.id.is_empty() {
        0.0
    } else {
        (profile.authors.get(&item.author.id).copied().unwrap_or(0.0) / profile.max_author.max(1.0))
            .min(1.0)
    };
    let quality = ((1.0 + item.vote_up_count as f32).ln() / 12.0).min(1.0);
    let freshness = 1.0 - position as f32 / (total - 1) as f32;
    0.60 * topic + 0.25 * author + 0.10 * quality + 0.05 * freshness
}

fn diversity_penalty(item: &FeedItem, output: &[FeedItem]) -> f32 {
    let mut penalty = 0.0;
    if let Some(last) = output.last() {
        if same_author(item, last) {
            penalty += 0.20;
        }
        penalty += 0.25 * topic_overlap(item, last);
    }
    if let Some(second_last) = output.iter().rev().nth(1) {
        if same_author(item, second_last) {
            // Strongly discourage three items from the same author in a row.
            penalty += 1.00;
        }
        penalty += 0.10 * topic_overlap(item, second_last);
    }
    penalty
}

fn same_author(a: &FeedItem, b: &FeedItem) -> bool {
    !a.author.id.is_empty() && a.author.id == b.author.id
}

fn topic_overlap(a: &FeedItem, b: &FeedItem) -> f32 {
    if a.topics.is_empty() || b.topics.is_empty() {
        return 0.0;
    }
    let shared = a
        .topics
        .iter()
        .filter(|topic| b.topics.contains(topic))
        .count();
    shared as f32 / a.topics.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use zhihu_types::{Author, ContentKind};

    fn item(id: &str, author: &str, topic: &str, votes: u64) -> FeedItem {
        FeedItem {
            id: id.to_string(),
            title: format!("标题 {id}"),
            excerpt: String::new(),
            author: Author {
                id: author.to_string(),
                name: author.to_string(),
                headline: None,
                url_token: None,
                is_following: false,
                avatar_url: None,
            },
            kind: ContentKind::Answer,
            vote_up_count: votes,
            comment_count: 1,
            topics: vec![topic.to_string()],
        }
    }

    fn profile(topics: &[(&str, f32)], authors: &[(&str, f32)], now: i64) -> InterestProfile {
        let mut records = Vec::new();
        for (topic, weight) in topics {
            records.push(FeedbackRecord {
                author_id: String::new(),
                topics: vec![topic.to_string()],
                weight: *weight,
                created_at: now,
            });
        }
        for (author, weight) in authors {
            records.push(FeedbackRecord {
                author_id: author.to_string(),
                topics: Vec::new(),
                weight: *weight,
                created_at: now,
            });
        }
        InterestProfile::from_feedback(&records, now)
    }

    #[test]
    fn mode_roundtrip() {
        for mode in [
            RecommendMode::Off,
            RecommendMode::Hybrid,
            RecommendMode::Local,
        ] {
            assert_eq!(RecommendMode::parse(mode.as_str()), mode);
        }
        assert_eq!(RecommendMode::parse("unknown"), RecommendMode::Off);
    }

    #[test]
    fn empty_profile_keeps_server_order() {
        let mut items = vec![item("1", "a", "rust", 10), item("2", "b", "go", 10)];
        rerank(
            &mut items,
            &InterestProfile::default(),
            RecommendMode::Local,
        );
        assert_eq!(items[0].id, "1");
        assert_eq!(items[1].id, "2");
    }

    #[test]
    fn topic_match_moves_to_the_front() {
        let now = 1_000_000;
        let profile = profile(&[("rust", 3.0)], &[], now);
        let mut items = vec![
            item("1", "a", "go", 100),
            item("2", "b", "rust", 1),
            item("3", "c", "python", 50),
        ];
        rerank(&mut items, &profile, RecommendMode::Local);
        assert_eq!(items[0].id, "2");
    }

    #[test]
    fn hybrid_keeps_some_server_order() {
        let now = 1_000_000;
        let profile = profile(&[("rust", 0.1)], &[], now);
        let mut items = vec![
            item("1", "a", "go", 100),
            item("2", "b", "rust", 1),
            item("3", "c", "python", 50),
        ];
        rerank(&mut items, &profile, RecommendMode::Hybrid);
        assert_eq!(items.len(), 3);
        // The server-top item still outranks the weakly matching second item.
        assert_eq!(items[0].id, "1");
        assert_eq!(items[1].id, "2");
    }

    #[test]
    fn diversity_breaks_author_runs() {
        let now = 1_000_000;
        let profile = profile(&[], &[("a", 5.0)], now);
        let mut items = vec![
            item("1", "a", "rust", 10),
            item("2", "a", "go", 10),
            item("3", "a", "python", 10),
            item("4", "b", "rust", 10),
        ];
        rerank(&mut items, &profile, RecommendMode::Local);
        // Two same-author items may lead, but not three.
        assert!(items[0].author.id == "a");
        assert!(items[1].author.id == "a");
        assert_eq!(items[2].author.id, "b");
    }

    #[test]
    fn title_tokens_match_similar_titles() {
        let tokens = title_tokens("Rust 如何评价 GPUI 移动端?");
        assert!(tokens.contains(&"t:rust".to_string()));
        assert!(tokens.contains(&"t:gpui".to_string()));
        assert!(tokens.iter().any(|token| token == "t:移动"));
        assert!(!tokens.contains(&"t:如何".to_string()));

        let now = 1_000_000;
        let mut first = item("1", "a", "go", 10);
        first.title = "Rust 移动端 UI 实践".to_string();
        first.topics.clear();
        let mut second = item("2", "b", "go", 10);
        second.title = "Rust 移动端性能优化".to_string();
        second.topics.clear();
        let mut unrelated = item("3", "c", "go", 10);
        unrelated.title = "今天吃什么".to_string();
        unrelated.topics.clear();

        let profile = InterestProfile::from_feedback(
            &[FeedbackRecord {
                author_id: String::new(),
                topics: title_tokens(&first.title),
                weight: 4.0,
                created_at: now,
            }],
            now,
        );
        let mut items = vec![unrelated, second, first];
        rerank(&mut items, &profile, RecommendMode::Local);
        assert_eq!(items[0].id, "1");
        assert_eq!(items[2].id, "3");
    }

    #[test]
    fn dislike_subtracts_interest() {
        let now = 1_000_000;
        let profile = InterestProfile::from_feedback(
            &[
                FeedbackRecord {
                    author_id: "a".to_string(),
                    topics: vec!["rust".to_string()],
                    weight: 4.0,
                    created_at: now,
                },
                FeedbackRecord {
                    author_id: "a".to_string(),
                    topics: vec!["rust".to_string()],
                    weight: -1.5,
                    created_at: now,
                },
            ],
            now,
        );
        assert!((profile.topics["rust"] - 2.5).abs() < 0.01);
        assert!((profile.authors["a"] - 2.5).abs() < 0.01);
    }

    #[test]
    fn old_feedback_decays() {
        let now = 10_000_000;
        let fresh = InterestProfile::from_feedback(
            &[FeedbackRecord {
                author_id: "a".to_string(),
                topics: vec!["rust".to_string()],
                weight: 4.0,
                created_at: now,
            }],
            now,
        );
        let old = InterestProfile::from_feedback(
            &[FeedbackRecord {
                author_id: "a".to_string(),
                topics: vec!["rust".to_string()],
                weight: 4.0,
                created_at: now - 90 * 86_400,
            }],
            now,
        );
        assert!(fresh.topics["rust"] > old.topics["rust"]);
        assert!(old.topics["rust"] > 0.0);
    }
}
