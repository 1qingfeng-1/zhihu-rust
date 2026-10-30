//! Domain types shared by the API, core and UI layers.
//!
//! DTOs mirroring the Zhihu wire format live in [`dto`] (added in M1); these
//! types are the stable, UI-facing representation.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub headline: Option<String>,
    #[serde(default)]
    pub url_token: Option<String>,
    #[serde(default)]
    pub is_following: bool,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub is_mcn: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentKind {
    Answer,
    Article,
    Pin,
    Question,
}

impl ContentKind {
    pub fn label(self) -> &'static str {
        match self {
            ContentKind::Answer => "回答",
            ContentKind::Article => "文章",
            ContentKind::Pin => "想法",
            ContentKind::Question => "问题",
        }
    }

    /// Stable lowercase key used by the local SQLite store.
    pub fn as_str(self) -> &'static str {
        match self {
            ContentKind::Answer => "answer",
            ContentKind::Article => "article",
            ContentKind::Pin => "pin",
            ContentKind::Question => "question",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "answer" => Some(ContentKind::Answer),
            "article" => Some(ContentKind::Article),
            "pin" => Some(ContentKind::Pin),
            "question" => Some(ContentKind::Question),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotSearchItem {
    pub id: String,
    pub query: String,
    pub hot_show: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyStory {
    pub id: u64,
    pub title: String,
    pub hint: String,
    pub url: String,
    pub image: Option<String>,
    #[serde(default)]
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyDetail {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub body_html: String,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    pub id: String,
    pub name: String,
    pub url_token: Option<String>,
    pub avatar_url: Option<String>,
    pub headline: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentDetail {
    pub id: String,
    pub title: String,
    pub author: Author,
    pub kind: ContentKind,
    pub content_html: String,
    pub excerpt: String,
    pub vote_up_count: u64,
    pub comment_count: u64,
    #[serde(default = "neutral_vote")]
    pub vote_state: String,
    #[serde(default)]
    pub created_time: i64,
    #[serde(default)]
    pub updated_time: i64,
    #[serde(default)]
    pub question_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FollowFeedKind {
    #[default]
    Recommend,
    Activity,
}

impl FollowFeedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Recommend => "推荐",
            Self::Activity => "动态",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub item_count: u64,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_favorited: bool,
}

/// A Zhihu column (`members/{token}/column-contributions`).
///
/// There is no in-app column page yet; the profile tab lists columns and opens
/// them in the browser, matching the reference client.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberColumn {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub article_count: u64,
    #[serde(default)]
    pub follower_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicDetail {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub followers_count: u64,
    #[serde(default)]
    pub questions_count: u64,
    #[serde(default)]
    pub discuss_count: u64,
    #[serde(default)]
    pub is_following: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopicFeedTab {
    #[default]
    Discussion,
    Ideas,
    Unanswered,
}

impl TopicFeedTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discussion => "讨论",
            Self::Ideas => "想法",
            Self::Unanswered => "待回答",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopicSort {
    #[default]
    Essence,
    Hot,
    Latest,
}

impl TopicSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Essence => "精华",
            Self::Hot => "最热",
            Self::Latest => "最新",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail_html: String,
    #[serde(default)]
    pub answer_count: u64,
    #[serde(default)]
    pub follower_count: u64,
}

fn neutral_vote() -> String {
    "neutral".to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub content: String,
    pub author: Author,
    pub reply_to: Option<String>,
    pub like_count: u64,
    pub child_count: u64,
    pub created_time: i64,
    pub is_author: bool,
    #[serde(default)]
    pub is_mine: bool,
    #[serde(default)]
    pub root_id: Option<String>,
    #[serde(default)]
    pub children: Vec<Comment>,
    #[serde(default)]
    pub child_next_offset: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentsPage {
    pub items: Vec<Comment>,
    pub is_end: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// `answer` or `article`.
    pub kind: String,
    pub id: String,
    pub title: String,
    pub read_at: i64,
    pub progress: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberProfile {
    pub id: String,
    pub name: String,
    pub url_token: String,
    pub avatar_url: Option<String>,
    pub headline: Option<String>,
    pub description: String,
    pub follower_count: u64,
    pub following_count: u64,
    pub answer_count: u64,
    pub articles_count: u64,
    pub is_following: bool,
    #[serde(default)]
    pub is_blocking: bool,
    #[serde(default)]
    pub is_mcn: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentAnchor {
    pub kind: ContentKind,
    pub content_id: String,
    pub comment_id: String,
    pub is_child: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationEntry {
    pub id: String,
    pub title: String,
    pub text: String,
    pub actor: String,
    #[serde(default)]
    pub actor_token: Option<String>,
    #[serde(default)]
    pub actor_avatar: Option<String>,
    pub created: i64,
    pub is_read: bool,
    #[serde(default)]
    pub target_link: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
}

impl NotificationEntry {
    /// Parse the official `zhihu://comment/list/...` target used by reply and
    /// comment-like notifications.
    pub fn comment_anchor(&self) -> Option<CommentAnchor> {
        let link = self.target_link.as_deref()?;
        parse_comment_anchor(link)
    }

    pub fn content_target(&self) -> Option<(ContentKind, String)> {
        parse_content_link(self.target_url.as_deref()?)
            .or_else(|| parse_content_link(self.target_link.as_deref()?))
    }
}

pub fn parse_comment_anchor(link: &str) -> Option<CommentAnchor> {
    let rest = link
        .strip_prefix("zhihu://comment/list/")
        .or_else(|| link.split("/comment/list/").nth(1))?;
    let (path, query_string) = rest.split_once('?').unwrap_or((rest, ""));
    let mut parts = path.split('/');
    let kind = match parts.next()? {
        "answer" => ContentKind::Answer,
        "article" => ContentKind::Article,
        "pin" => ContentKind::Pin,
        "question" => ContentKind::Question,
        _ => return None,
    };
    let content_id = parts.next()?.trim().to_string();
    if content_id.is_empty() {
        return None;
    }
    let mut comment_id = None;
    let mut is_child = false;
    for pair in query_string.split('&') {
        let (key, value) = pair.split_once('=')?;
        match key {
            "anchor_comment_id" => comment_id = Some(value.to_string()),
            "is_child" => is_child = value == "true" || value == "1",
            _ => {}
        }
    }
    Some(CommentAnchor {
        kind,
        content_id,
        comment_id: comment_id?,
        is_child,
    })
}

fn parse_content_link(link: &str) -> Option<(ContentKind, String)> {
    if let Some(rest) = link.split("/answer/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| (ContentKind::Answer, id.to_string()));
    }
    if let Some(rest) = link.split("zhuanlan.zhihu.com/p/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| (ContentKind::Article, id.to_string()));
    }
    if let Some(rest) = link.split("/pin/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| (ContentKind::Pin, id.to_string()));
    }
    if let Some(rest) = link.split("/question/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| (ContentKind::Question, id.to_string()));
    }
    None
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    pub id: i64,
    /// `answer`, `article`, `pin` or `question`.
    pub target_type: String,
    pub target_id: String,
    pub title: String,
    pub quote: String,
    pub note: String,
    pub tags: String,
    pub color: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedItem {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub author: Author,
    pub kind: ContentKind,
    pub vote_up_count: u64,
    pub comment_count: u64,
    /// Publish time (unix seconds) as reported by the server; 0 when the
    /// source does not provide one.
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub topic_refs: Vec<TopicRef>,
}

/// Placeholder dataset used by the M0 smoke screen.
pub fn m0_sample_feed() -> Vec<FeedItem> {
    const TITLES: &[(&str, &str, &str, ContentKind, u64, u64)] = &[
        (
            "如何评价 Rust 在移动端 UI 领域的现状？",
            "最近在用 GPUI 做一个移动端应用，想聊聊跨平台渲染、输入法和生态的体验。",
            "虚构的知乎用户",
            ContentKind::Answer,
            1284,
            96,
        ),
        (
            "为什么越来越多人选择自己写客户端？",
            "广告、推荐算法、隐私，这三个词大概能概括大部分人的理由。",
            "产品观察员",
            ContentKind::Article,
            932,
            41,
        ),
        (
            "用 Rust 重写一个 App，真正的难点在哪里？",
            "语言本身不是问题，难的是平台能力、富文本渲染和长期维护。",
            "一位路过的工程师",
            ContentKind::Answer,
            2107,
            158,
        ),
        (
            "2026 年，端侧 AI 能做什么？",
            "从语义过滤到本地摘要，端侧模型正在变得可用。",
            "边缘计算笔记",
            ContentKind::Article,
            671,
            23,
        ),
        (
            "有哪些看似小众但很好用的信息流阅读技巧？",
            "关注少数高质量作者，比追逐热榜更有效。",
            "信息食谱",
            ContentKind::Pin,
            458,
            12,
        ),
        (
            "GPUI 的虚拟列表是怎么做到高性能的？",
            "只布局可见区域，测量一次即可复用，这是 uniform_list 的核心。",
            "渲染管线",
            ContentKind::Answer,
            1543,
            77,
        ),
        (
            "移动端中文排版有哪些容易忽略的细节？",
            "行高、标点挤压、字体回退，都会影响阅读体验。",
            "字体与排版",
            ContentKind::Article,
            889,
            34,
        ),
        (
            "一个客户端应该保留多少功能？",
            "功能不是越多越好，稳定、快速、可预期才是长期价值。",
            "少即是多",
            ContentKind::Pin,
            312,
            9,
        ),
        (
            "如何设计一套可测试的客户端架构？",
            "把网络、存储、状态机与 UI 解耦，测试就不再依赖设备。",
            "架构手记",
            ContentKind::Answer,
            1260,
            64,
        ),
        (
            "知乎的 API 签名机制是如何工作的？",
            "x-zse-93 / x-zse-96 与请求指纹，是客户端需要持续维护的部分。",
            "协议研究",
            ContentKind::Article,
            745,
            28,
        ),
        (
            "为什么阅读类 App 的缓存策略特别重要？",
            "弱网和地铁场景决定了体验下限。",
            "移动开发札记",
            ContentKind::Answer,
            534,
            19,
        ),
        (
            "从 Compose 迁移到原生渲染是一种什么体验？",
            "布局思维接近，但生态、调试和平台桥接需要重新建立。",
            "跨端实践",
            ContentKind::Article,
            1082,
            52,
        ),
    ];

    // Sample items get synthetic publish times a few hours apart so the
    // Android timeline has something meaningful to show.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    TITLES
        .iter()
        .enumerate()
        .map(
            |(i, (title, excerpt, author, kind, votes, comments))| FeedItem {
                id: format!("m0-{i}"),
                title: (*title).to_string(),
                excerpt: (*excerpt).to_string(),
                author: Author {
                    id: String::new(),
                    name: (*author).to_string(),
                    headline: None,
                    url_token: None,
                    is_following: false,
                    avatar_url: None,
                    is_mcn: false,
                },
                kind: *kind,
                vote_up_count: *votes,
                comment_count: *comments,
                created_at: now - (i as i64) * 3 * 3_600 - 900,
                topics: Vec::new(),
                topic_refs: Vec::new(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comment_notification_anchor() {
        let anchor =
            parse_comment_anchor("zhihu://comment/list/answer/2?anchor_comment_id=3&is_child=true")
                .unwrap();
        assert_eq!(anchor.kind, ContentKind::Answer);
        assert_eq!(anchor.content_id, "2");
        assert_eq!(anchor.comment_id, "3");
        assert!(anchor.is_child);

        assert!(parse_comment_anchor("zhihu://comment/list/answer/2").is_none());
        assert!(parse_comment_anchor("https://www.zhihu.com/people/x").is_none());
    }

    #[test]
    fn resolves_notification_content_target() {
        assert_eq!(
            parse_content_link("https://www.zhihu.com/question/1/answer/2"),
            Some((ContentKind::Answer, "2".to_string()))
        );
        assert_eq!(
            parse_content_link("https://www.zhihu.com/pin/9"),
            Some((ContentKind::Pin, "9".to_string()))
        );
    }
}
