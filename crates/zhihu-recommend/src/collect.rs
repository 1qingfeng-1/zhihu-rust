//! Local recommendation candidate pool (M8-1).
//!
//! Candidates come only from list surfaces the user already browses and are
//! stored as metadata: titles, excerpts, author, topics and counters. Bodies,
//! cookies and comments never enter the pool, and nothing is uploaded.
//!
//! This module owns the pool model, the manual-collection policy and the
//! pre-insert filters. Network orchestration lives in `zhihu-core`; persistence
//! lives in `zhihu-store`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use zhihu_types::{Author, ContentKind, DailyStory, FeedItem};

pub const DEFAULT_MAX_PAGES: u32 = 3;
pub const HARD_MAX_PAGES: u32 = 5;
pub const DEFAULT_RETENTION_DAYS: i64 = 7;
pub const DEFAULT_MAX_ITEMS: usize = 2000;
pub const DEFAULT_THROTTLE: Duration = Duration::from_secs(2);

/// Pool kind. Mirrors [`ContentKind`] and adds the daily section, which has no
/// content route of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    Answer,
    Article,
    Pin,
    Question,
    Daily,
}

impl CandidateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CandidateKind::Answer => "answer",
            CandidateKind::Article => "article",
            CandidateKind::Pin => "pin",
            CandidateKind::Question => "question",
            CandidateKind::Daily => "daily",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "answer" => Some(CandidateKind::Answer),
            "article" => Some(CandidateKind::Article),
            "pin" => Some(CandidateKind::Pin),
            "question" => Some(CandidateKind::Question),
            "daily" => Some(CandidateKind::Daily),
            _ => None,
        }
    }
}

impl From<ContentKind> for CandidateKind {
    fn from(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Answer => CandidateKind::Answer,
            ContentKind::Article => CandidateKind::Article,
            ContentKind::Pin => CandidateKind::Pin,
            ContentKind::Question => CandidateKind::Question,
        }
    }
}

/// One item in the local pool. `(kind, id)` is the dedupe key.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub kind: CandidateKind,
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub author_id: String,
    pub author_name: String,
    pub topics: Vec<String>,
    pub vote_up_count: u64,
    pub comment_count: u64,
    pub is_mcn: bool,
    /// Where the item was seen, e.g. `recommend`, `topic:123`.
    pub source: String,
    pub collected_at: i64,
    pub expires_at: i64,
}

impl Candidate {
    /// Map a feed card. Items without an id cannot be routed later, so they are
    /// dropped instead of stored.
    pub fn from_feed_item(
        item: &FeedItem,
        source: &str,
        now: i64,
        retention_days: i64,
    ) -> Option<Self> {
        let id = item.id.trim();
        if id.is_empty() {
            return None;
        }
        Some(Self {
            kind: CandidateKind::from(item.kind),
            id: id.to_string(),
            title: item.title.trim().to_string(),
            excerpt: item.excerpt.trim().to_string(),
            author_id: item.author.id.trim().to_string(),
            author_name: item.author.name.trim().to_string(),
            topics: clean_topics(&item.topics),
            vote_up_count: item.vote_up_count,
            comment_count: item.comment_count,
            is_mcn: item.author.is_mcn,
            source: source.to_string(),
            collected_at: now,
            expires_at: expires_at(now, retention_days),
        })
    }

    pub fn from_daily_story(story: &DailyStory, now: i64, retention_days: i64) -> Option<Self> {
        if story.id == 0 || story.title.trim().is_empty() {
            return None;
        }
        Some(Self {
            kind: CandidateKind::Daily,
            id: story.id.to_string(),
            title: story.title.trim().to_string(),
            excerpt: story.hint.trim().to_string(),
            author_id: String::new(),
            author_name: String::new(),
            topics: Vec::new(),
            vote_up_count: 0,
            comment_count: 0,
            is_mcn: false,
            source: "daily".to_string(),
            collected_at: now,
            expires_at: expires_at(now, retention_days),
        })
    }

    /// Map back to a feed card for the local feed. Daily stories return `None`:
    /// they only exist inside the daily section and have no content route.
    pub fn to_feed_item(&self) -> Option<FeedItem> {
        let kind = match self.kind {
            CandidateKind::Answer => ContentKind::Answer,
            CandidateKind::Article => ContentKind::Article,
            CandidateKind::Pin => ContentKind::Pin,
            CandidateKind::Question => ContentKind::Question,
            CandidateKind::Daily => return None,
        };
        Some(FeedItem {
            id: self.id.clone(),
            title: self.title.clone(),
            excerpt: self.excerpt.clone(),
            author: Author {
                id: self.author_id.clone(),
                name: if self.author_name.is_empty() {
                    "知乎用户".to_string()
                } else {
                    self.author_name.clone()
                },
                headline: None,
                url_token: None,
                is_following: false,
                avatar_url: None,
                is_mcn: self.is_mcn,
            },
            kind,
            vote_up_count: self.vote_up_count,
            comment_count: self.comment_count,
            created_at: 0,
            topics: self.topics.clone(),
            topic_refs: Vec::new(),
        })
    }
}

pub fn expires_at(collected_at: i64, retention_days: i64) -> i64 {
    collected_at + retention_days.max(1) * 86_400
}

fn clean_topics(topics: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    topics
        .iter()
        .map(|topic| topic.trim())
        .filter(|topic| !topic.is_empty())
        .filter(|topic| seen.insert((*topic).to_string()))
        .map(str::to_string)
        .collect()
}

/// Surfaces the manual collector may read. Anything else (search, comments,
/// private messages) is deliberately not representable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectSource {
    RecommendFeed,
    FollowingFeed,
    Daily,
    Topic { id: String },
    Member { token: String },
}

impl CollectSource {
    /// Provenance string stored with every candidate.
    pub fn slug(&self) -> String {
        match self {
            CollectSource::RecommendFeed => "recommend".to_string(),
            CollectSource::FollowingFeed => "following".to_string(),
            CollectSource::Daily => "daily".to_string(),
            CollectSource::Topic { id } => format!("topic:{id}"),
            CollectSource::Member { token } => format!("member:{token}"),
        }
    }

    pub fn label(&self) -> String {
        match self {
            CollectSource::RecommendFeed => "推荐流".to_string(),
            CollectSource::FollowingFeed => "关注动态".to_string(),
            CollectSource::Daily => "日报".to_string(),
            CollectSource::Topic { id } => format!("话题 {id}"),
            CollectSource::Member { token } => format!("用户 {token}"),
        }
    }

    /// Sources that need no extra identifiers. Used by the manual trigger.
    pub fn manual_defaults() -> Vec<Self> {
        vec![
            CollectSource::RecommendFeed,
            CollectSource::FollowingFeed,
            CollectSource::Daily,
        ]
    }
}

/// Collection limits. Always run through [`Self::sanitized`] before use so a
/// bad setting cannot turn a manual run into a crawl.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectorPolicy {
    pub max_pages: u32,
    pub throttle: Duration,
    pub retention_days: i64,
    pub max_items: usize,
}

impl Default for CollectorPolicy {
    fn default() -> Self {
        Self {
            max_pages: DEFAULT_MAX_PAGES,
            throttle: DEFAULT_THROTTLE,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_items: DEFAULT_MAX_ITEMS,
        }
    }
}

impl CollectorPolicy {
    pub fn sanitized(&self) -> Self {
        Self {
            max_pages: self.max_pages.clamp(1, HARD_MAX_PAGES),
            throttle: self
                .throttle
                .clamp(Duration::from_secs(1), Duration::from_secs(60)),
            retention_days: self.retention_days.clamp(1, 30),
            max_items: self.max_items.clamp(50, DEFAULT_MAX_ITEMS),
        }
    }
}

/// Cooperative cancellation. The UI cancels a run; the loop checks the flag
/// before every request and after every page.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Hard filters applied before insertion. Blocks are authoritative; MCN is only
/// skipped when the user enabled the existing "hide MCN" setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenRules {
    pub user_ids: Vec<String>,
    pub topic_names: Vec<String>,
    pub hide_mcn: bool,
}

impl HiddenRules {
    pub fn hides(&self, candidate: &Candidate) -> bool {
        if self.hide_mcn && candidate.is_mcn {
            return true;
        }
        if !candidate.author_id.is_empty()
            && self.user_ids.iter().any(|id| id == &candidate.author_id)
        {
            return true;
        }
        candidate.topics.iter().any(|topic| {
            self.topic_names
                .iter()
                .any(|blocked| blocked == topic || blocked == topic.trim())
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollectReport {
    pub sources: Vec<String>,
    pub pages: u32,
    pub fetched: usize,
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub pruned: usize,
    pub cancelled: bool,
    pub error: Option<String>,
}

impl CollectReport {
    pub fn succeeded(&self) -> bool {
        self.error.is_none() && !self.cancelled
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecommendPoolStats {
    pub items: usize,
    pub oldest_collected_at: Option<i64>,
    pub newest_collected_at: Option<i64>,
    pub approx_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zhihu_types::{Author, ContentKind};

    fn feed_item(id: &str) -> FeedItem {
        FeedItem {
            id: id.to_string(),
            title: "  如何评价 Rust？ ".to_string(),
            excerpt: " 移动端实践 ".to_string(),
            author: Author {
                id: "u1".to_string(),
                name: "alice".to_string(),
                headline: None,
                url_token: Some("alice".to_string()),
                is_following: false,
                avatar_url: None,
                is_mcn: false,
            },
            kind: ContentKind::Answer,
            vote_up_count: 12,
            comment_count: 3,
            created_at: 0,
            topics: vec![
                "Rust".to_string(),
                " ".to_string(),
                "Rust".to_string(),
                "GPUI".to_string(),
            ],
            topic_refs: Vec::new(),
        }
    }

    #[test]
    fn maps_feed_items_and_expiry() {
        let candidate = Candidate::from_feed_item(&feed_item("42"), "recommend", 1_000, 7).unwrap();
        assert_eq!(candidate.kind, CandidateKind::Answer);
        assert_eq!(candidate.id, "42");
        assert_eq!(candidate.title, "如何评价 Rust？");
        assert_eq!(candidate.excerpt, "移动端实践");
        assert_eq!(
            candidate.topics,
            vec!["Rust".to_string(), "GPUI".to_string()]
        );
        assert_eq!(candidate.source, "recommend");
        assert_eq!(candidate.collected_at, 1_000);
        assert_eq!(candidate.expires_at, 1_000 + 7 * 86_400);
    }

    #[test]
    fn drops_items_without_id() {
        assert!(Candidate::from_feed_item(&feed_item("  "), "recommend", 1, 7).is_none());
    }

    #[test]
    fn maps_daily_stories() {
        let story = DailyStory {
            id: 9,
            title: "今日事".to_string(),
            hint: "摘要".to_string(),
            url: String::new(),
            image: None,
            date: "20261030".to_string(),
        };
        let candidate = Candidate::from_daily_story(&story, 5, 7).unwrap();
        assert_eq!(candidate.kind, CandidateKind::Daily);
        assert_eq!(candidate.id, "9");
        assert_eq!(candidate.source, "daily");
        assert_eq!(candidate.expires_at, 5 + 7 * 86_400);
    }

    #[test]
    fn policy_is_clamped_for_safety() {
        let policy = CollectorPolicy {
            max_pages: 99,
            throttle: Duration::from_millis(1),
            retention_days: 365,
            max_items: 999_999,
        }
        .sanitized();
        assert_eq!(policy.max_pages, HARD_MAX_PAGES);
        assert_eq!(policy.throttle, Duration::from_secs(1));
        assert_eq!(policy.retention_days, 30);
        assert_eq!(policy.max_items, DEFAULT_MAX_ITEMS);

        let floor = CollectorPolicy {
            max_pages: 0,
            throttle: Duration::from_secs(600),
            retention_days: 0,
            max_items: 0,
        }
        .sanitized();
        assert_eq!(floor.max_pages, 1);
        assert_eq!(floor.throttle, Duration::from_secs(60));
        assert_eq!(floor.retention_days, 1);
        assert_eq!(floor.max_items, 50);
    }

    #[test]
    fn hidden_rules_filter_users_topics_and_mcn() {
        let mut candidate = Candidate::from_feed_item(&feed_item("1"), "recommend", 0, 7).unwrap();
        let users = HiddenRules {
            user_ids: vec!["u1".to_string()],
            ..Default::default()
        };
        assert!(users.hides(&candidate));

        let topics = HiddenRules {
            topic_names: vec!["GPUI".to_string()],
            ..Default::default()
        };
        assert!(topics.hides(&candidate));

        candidate.is_mcn = true;
        assert!(!HiddenRules::default().hides(&candidate));
        assert!(HiddenRules {
            hide_mcn: true,
            ..Default::default()
        }
        .hides(&candidate));
        assert!(!HiddenRules {
            user_ids: vec!["other".to_string()],
            topic_names: vec!["Rust 2026".to_string()],
            hide_mcn: false,
        }
        .hides(&candidate));
    }

    #[test]
    fn cancel_token_is_shared() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn source_slugs_are_stable() {
        assert_eq!(CollectSource::RecommendFeed.slug(), "recommend");
        assert_eq!(CollectSource::FollowingFeed.slug(), "following");
        assert_eq!(CollectSource::Daily.slug(), "daily");
        assert_eq!(CollectSource::Topic { id: "7".into() }.slug(), "topic:7");
        assert_eq!(
            CollectSource::Member {
                token: "alice".into()
            }
            .slug(),
            "member:alice"
        );
        assert_eq!(CollectSource::manual_defaults().len(), 3);
    }

    #[test]
    fn candidate_kind_roundtrip() {
        for kind in [
            CandidateKind::Answer,
            CandidateKind::Article,
            CandidateKind::Pin,
            CandidateKind::Question,
            CandidateKind::Daily,
        ] {
            assert_eq!(CandidateKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(CandidateKind::parse("video"), None);
        assert_eq!(
            CandidateKind::from(ContentKind::Question),
            CandidateKind::Question
        );
    }
}
