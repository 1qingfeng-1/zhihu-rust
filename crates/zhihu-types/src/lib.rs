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
    #[serde(default)]
    pub topics: Vec<String>,
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
                },
                kind: *kind,
                vote_up_count: *votes,
                comment_count: *comments,
                topics: Vec::new(),
            },
        )
        .collect()
}
