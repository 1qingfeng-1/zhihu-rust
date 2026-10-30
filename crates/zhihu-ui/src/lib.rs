//! GPUI views and components for the Zhihu client.
//!
//! M1 adds a real data path: the hot-search and Zhihu Daily tabs are backed by
//! public HTTP endpoints (no login required).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

mod selectable;

use gpui_kit::base::TextSelection;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, img, linear_color_stop, linear_gradient, px, rgb, rgba, uniform_list, Animation,
    AnimationExt as _, AnyElement, ClipboardItem, Context, DispatchPhase, Div, Entity, FocusHandle,
    Focusable, FontWeight, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, ObjectFit,
    Render, Rgba, ScrollDelta, ScrollHandle, ScrollStrategy, ScrollWheelEvent, SharedString,
    TouchPhase, UniformListScrollHandle, WeakEntity, Window,
};
use zhihu_content::Block;
use zhihu_core::{
    AppServices, AppSettings, CollectRequest, SearchOptions, SearchSort, SearchVertical, UiEvent,
};
use zhihu_recommend::{
    rank_pool_with_vectors, rerank_with_vectors, title_tokens, CancelToken, CollectReport,
    InterestProfile, RecommendMode, RecommendPoolStats, RecommendPreset, VectorProfile,
};
use zhihu_types::{
    m0_sample_feed, AccountProfile, Author, ContentDetail, ContentKind, DailyStory, FeedItem,
    FollowFeedKind, HotSearchItem, TopicDetail, TopicFeedTab, TopicSort,
};

/// In-app Zhihu login / risk-verification WebView bridge (Android).
#[cfg(target_os = "android")]
pub mod android_login {
    use std::sync::Mutex;

    use jni::objects::JValue;

    /// Java host class for the in-app login / risk-check WebView.
    const ZHIHU_LOGIN_CLASS: &str = "dev.gpui.mobile.ZhihuLogin";

    #[derive(Clone, Debug)]
    pub enum ImportedCookies {
        /// Cookies from the sign-in WebView.
        WebLogin(String),
        /// Cookies read back from the risk-verification WebView.
        Session(String),
    }

    static PENDING: Mutex<Option<ImportedCookies>> = Mutex::new(None);

    pub fn store_cookie_header(raw: String) {
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(ImportedCookies::WebLogin(raw));
        }
    }

    pub fn store_session_cookies(raw: String) {
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(ImportedCookies::Session(raw));
        }
    }

    pub fn take_imported() -> Option<ImportedCookies> {
        PENDING.lock().ok().and_then(|mut slot| slot.take())
    }

    pub fn open_web_login() -> Result<(), String> {
        gpui_mobile::android::jni::with_env(|env| {
            // Native threads resolve `FindClass` through the system loader,
            // which cannot see app classes; go through the Activity's loader.
            let class = gpui_mobile::android::jni::find_app_class(env, ZHIHU_LOGIN_CLASS)?;
            let activity = gpui_mobile::android::jni::activity(env)?;
            env.call_static_method(
                &class,
                jni::jni_str!("openWebLogin"),
                jni::jni_sig!("(Landroid/app/Activity;)V"),
                &[JValue::Object(&activity)],
            )
            .map_err(|error| {
                env.exception_clear();
                error.to_string()
            })?;
            Ok(())
        })
    }

    pub fn open_risk_control(url: &str, pairs: &str) -> Result<(), String> {
        gpui_mobile::android::jni::with_env(|env| {
            let class = gpui_mobile::android::jni::find_app_class(env, ZHIHU_LOGIN_CLASS)?;
            let activity = gpui_mobile::android::jni::activity(env)?;
            let url = env.new_string(url).map_err(|error| error.to_string())?;
            let pairs = env.new_string(pairs).map_err(|error| error.to_string())?;
            env.call_static_method(
                &class,
                jni::jni_str!("openRiskControl"),
                jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&url),
                    JValue::Object(&pairs),
                ],
            )
            .map_err(|error| {
                env.exception_clear();
                error.to_string()
            })?;
            Ok(())
        })
    }
}

/// Back-stack depth exposed to the Android activity so the system back key can
/// decide between popping a route and backgrounding the app.
#[cfg(target_os = "android")]
pub mod android_nav {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DEPTH: AtomicUsize = AtomicUsize::new(0);

    pub fn set_back_depth(depth: usize) {
        DEPTH.store(depth, Ordering::Relaxed);
    }

    pub fn can_go_back() -> bool {
        DEPTH.load(Ordering::Relaxed) > 0
    }
}

#[cfg(target_os = "android")]
fn cookie_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

mod collections;
mod crash;
mod emoji;
mod emoji_data;
mod home;
mod pos;
mod render;
mod topics;
mod tts;
mod typography;

pub use crash::install_panic_hook;
pub use tts::{is_speaking, speak, stop_speaking};

use render::render_blocks;
use typography::{Typography, TypographyPreset};

#[cfg(target_os = "windows")]
pub(crate) const UI_FONT: &str = "Microsoft YaHei";
#[cfg(target_os = "macos")]
pub(crate) const UI_FONT: &str = "PingFang SC";
#[cfg(target_os = "linux")]
pub(crate) const UI_FONT: &str = "Noto Sans CJK SC";
#[cfg(target_os = "android")]
pub(crate) const UI_FONT: &str = "Noto Sans CJK SC";
#[cfg(target_os = "ios")]
pub(crate) const UI_FONT: &str = "PingFang SC";

#[cfg(target_os = "windows")]
pub(crate) const MONO_FONT: &str = "Consolas";
#[cfg(target_os = "macos")]
pub(crate) const MONO_FONT: &str = "Menlo";
#[cfg(target_os = "linux")]
pub(crate) const MONO_FONT: &str = "Noto Sans Mono CJK SC";
#[cfg(target_os = "android")]
pub(crate) const MONO_FONT: &str = "Droid Sans Mono";
#[cfg(target_os = "ios")]
pub(crate) const MONO_FONT: &str = "Menlo";

const TABS: [&str; 5] = ["推荐", "关注", "热榜", "日报", "我的"];
/// Local mode falls back to the server feed until the pool can fill a page.
const LOCAL_FEED_MIN_ITEMS: usize = 20;

#[derive(Clone, Debug, PartialEq)]
enum Route {
    Answer {
        id: String,
        title: String,
    },
    Article {
        id: String,
        title: String,
    },
    Pin {
        id: String,
        title: String,
    },
    Search {
        query: String,
        member: Option<(String, String)>,
    },
    Comments {
        kind: ContentKind,
        id: String,
        title: String,
        anchor: Option<String>,
    },
    Question {
        id: String,
        title: String,
    },
    Topic {
        id: String,
        name: String,
    },
    Daily {
        id: u64,
        title: String,
    },
    Member {
        token: String,
        name: String,
    },
    Collections,
    Collection {
        id: String,
        title: String,
    },
    FavoritePicker,
    Login,
    TtsSettings,
    Notifications,
    History,
    Notes,
    Crashes,
    Diagnostics,
    Other {
        title: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CommentSort {
    #[default]
    Default,
    Newest,
    Oldest,
    MostLiked,
}

impl CommentSort {
    fn label(self) -> &'static str {
        match self {
            CommentSort::Default => "默认",
            CommentSort::Newest => "最新",
            CommentSort::Oldest => "最早",
            CommentSort::MostLiked => "最多赞同",
        }
    }
}

/// One remembered comment page: the list plus where pagination stopped.
#[derive(Clone)]
struct CommentsCacheEntry {
    comments: Vec<zhihu_types::Comment>,
    offset: u32,
    end: bool,
}

fn comments_cache_key(kind: ContentKind, id: &str) -> String {
    format!("{kind:?}:{id}")
}

fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Stamp newly seen feed cards onto the local timeline. A refresh puts new
/// cards at "now"; loading more continues below the oldest stamp, so the rail
/// always reads newest-on-top however the server orders things. Cards already
/// stamped keep their place across refreshes.
fn stamp_feed_times(times: &mut HashMap<String, i64>, items: &[FeedItem], fresh: bool) {
    let now = unix_now_secs();
    let mut cursor = if fresh {
        now
    } else {
        times
            .values()
            .copied()
            .min()
            .unwrap_or(now)
            .saturating_sub(180)
    };
    for item in items {
        if !times.contains_key(&item.id) {
            times.insert(item.id.clone(), cursor);
        }
        cursor -= 180;
    }
    if times.len() > 600 {
        times.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum QuestionSort {
    #[default]
    Default,
    Newest,
    MostLiked,
}

impl QuestionSort {
    fn label(self) -> &'static str {
        match self {
            QuestionSort::Default => "默认",
            QuestionSort::Newest => "最新",
            QuestionSort::MostLiked => "最多赞同",
        }
    }

    fn order(self) -> &'static str {
        match self {
            QuestionSort::Default => "default",
            QuestionSort::Newest => "newest",
            QuestionSort::MostLiked => "liked",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MemberTab {
    #[default]
    Answers,
    Articles,
    Activities,
    Pins,
    Questions,
    Collections,
    Columns,
    Followers,
    Following,
}

impl MemberTab {
    const ALL: [MemberTab; 9] = [
        MemberTab::Answers,
        MemberTab::Articles,
        MemberTab::Activities,
        MemberTab::Pins,
        MemberTab::Questions,
        MemberTab::Collections,
        MemberTab::Columns,
        MemberTab::Followers,
        MemberTab::Following,
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default()
    }

    fn step(self, forward: bool) -> Option<Self> {
        let index = self.index();
        let next = if forward {
            (index + 1).min(Self::ALL.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        (next != index).then(|| Self::ALL[next])
    }

    fn label(self) -> &'static str {
        match self {
            MemberTab::Answers => "回答",
            MemberTab::Articles => "文章",
            MemberTab::Activities => "动态",
            MemberTab::Pins => "想法",
            MemberTab::Questions => "提问",
            MemberTab::Collections => "收藏",
            MemberTab::Columns => "专栏",
            MemberTab::Followers => "关注者",
            MemberTab::Following => "关注",
        }
    }

    fn content_kind(self) -> Option<ContentKind> {
        match self {
            MemberTab::Answers => Some(ContentKind::Answer),
            MemberTab::Articles => Some(ContentKind::Article),
            MemberTab::Pins => Some(ContentKind::Pin),
            MemberTab::Questions => Some(ContentKind::Question),
            _ => None,
        }
    }

    fn following(self) -> Option<bool> {
        match self {
            MemberTab::Followers => Some(false),
            MemberTab::Following => Some(true),
            _ => None,
        }
    }
}

fn sorted_comments(
    comments: &[zhihu_types::Comment],
    sort: CommentSort,
) -> Vec<zhihu_types::Comment> {
    let mut items = comments.to_vec();
    match sort {
        CommentSort::Default => {}
        CommentSort::Newest => items.sort_by_key(|comment| std::cmp::Reverse(comment.created_time)),
        CommentSort::Oldest => items.sort_by_key(|comment| comment.created_time),
        CommentSort::MostLiked => items.sort_by(|a, b| {
            b.like_count
                .cmp(&a.like_count)
                .then_with(|| b.created_time.cmp(&a.created_time))
        }),
    }
    items
}

#[derive(Clone, Debug)]
struct CommentReplyTarget {
    id: String,
    name: String,
    root_id: String,
}

#[derive(Default)]
struct FollowFeedState {
    items: Vec<FeedItem>,
    next: Option<String>,
    is_end: bool,
    loading: bool,
    loading_more: bool,
    error: Option<String>,
    initialized: bool,
}

impl FollowFeedState {
    fn apply_page(
        &mut self,
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
        next: Option<String>,
    ) {
        if append {
            let mut known: HashSet<String> =
                self.items.iter().map(|item| item.id.clone()).collect();
            for item in items {
                if known.insert(item.id.clone()) {
                    self.items.push(item);
                }
            }
        } else {
            self.items = items;
        }
        self.next = next;
        self.is_end = is_end;
        self.loading = false;
        self.loading_more = false;
        self.error = None;
        self.initialized = true;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FeedSource {
    #[default]
    Server,
    Hybrid,
    Local,
    /// Local mode requested but the pool is too small; server feed shown.
    LocalFallback,
}

impl FeedSource {
    fn label(self) -> &'static str {
        match self {
            FeedSource::Server => "服务端",
            FeedSource::Hybrid => "混合",
            FeedSource::Local => "本地推荐",
            FeedSource::LocalFallback => "服务端（本地池不足）",
        }
    }
}

#[derive(Clone, Debug)]
struct DiagSnapshot {
    uptime: std::time::Duration,
    data_dir: PathBuf,
    total_bytes: u64,
    image_files: usize,
    image_bytes: u64,
    crash_logs: usize,
    recommend_mode: RecommendMode,
    recommend_preset: RecommendPreset,
    recommend_feedback: usize,
    recommend_pool_items: usize,
    recommend_vectors: usize,
    recommend_model: String,
    recommend_embedding_fallback: bool,
    feed_source: FeedSource,
    feed_ms: Option<u128>,
    detail_ms: Option<u128>,
}

impl Route {
    fn title(&self) -> &str {
        match self {
            Route::Answer { title, .. }
            | Route::Article { title, .. }
            | Route::Pin { title, .. }
            | Route::Other { title } => title,
            Route::Search { query, .. } => query,
            Route::Comments { title, .. } => title,
            Route::Question { title, .. } => title,
            Route::Topic { name, .. } => name,
            Route::Daily { title, .. } => title,
            Route::Member { name, .. } => name,
            Route::Collections => "我的收藏",
            Route::Collection { title, .. } => title,
            Route::FavoritePicker => "选择收藏夹",
            Route::Login => "登录",
            Route::TtsSettings => "朗读设置",
            Route::Notifications => "消息",
            Route::History => "阅读历史",
            Route::Notes => "阅读笔记",
            Route::Crashes => "崩溃日志",
            Route::Diagnostics => "运行诊断",
        }
    }

    fn id(&self) -> Option<&str> {
        match self {
            Route::Answer { id, .. } | Route::Article { id, .. } | Route::Pin { id, .. } => {
                Some(id)
            }
            Route::Search { .. }
            | Route::Comments { .. }
            | Route::Question { .. }
            | Route::Topic { .. }
            | Route::Daily { .. }
            | Route::Member { .. }
            | Route::Collections
            | Route::Collection { .. }
            | Route::FavoritePicker
            | Route::Login
            | Route::TtsSettings
            | Route::Notifications
            | Route::History
            | Route::Notes
            | Route::Crashes
            | Route::Diagnostics
            | Route::Other { .. } => None,
        }
    }
}

/// Content route target used when returning from comments or other child
/// pages. `None` means the page below is not an answer/article.
fn route_content_target(route: &Route) -> Option<(ContentKind, String)> {
    match route {
        Route::Answer { id, .. } => Some((ContentKind::Answer, id.clone())),
        Route::Article { id, .. } => Some((ContentKind::Article, id.clone())),
        Route::Pin { id, .. } => Some((ContentKind::Pin, id.clone())),
        _ => None,
    }
}

const DEMO_HTML: &str = r#"
<h2>为什么用 Rust 重写一个客户端？</h2>
<p><img src="https://www.zhihu.com/equation?tex=%5Cfrac%7B-b%5Cpm%5Csqrt%7Bb%5E2-4ac%7D%7D%7B2a%7D" alt="-b±√(b²-4ac) / 2a"/></p>
<p>这是 M2 的富文本渲染示例，包含 <b>粗体</b>、<i>斜体</i>、<a href="https://www.rust-lang.org">链接</a>，以及行内公式 <img src="https://www.zhihu.com/equation?tex=E%3Dmc%5E2" alt="E=mc^2"/>。</p>
<blockquote><p>把网络、存储、状态机与 UI 解耦，测试就不再依赖设备。</p></blockquote>
<ul>
  <li>段落、标题与引用</li>
  <li>有序 / 无序列表</li>
  <li>代码块与图片占位</li>
</ul>
<pre><code class="language-rust">fn main() {
    println!("hello, zhihu");
}</code></pre>
<hr/>
<p>长文滚动测试。客户端最难的部分通常不是语言，而是平台能力、富文本渲染、输入法和长期维护。</p>
<p>这一段用于验证多段落的排版、行高和自动换行。中文排版需要关注标点挤压、字体回退和可读性。</p>
<p>下面是一张远程图片，已接入异步加载（磁盘缓存待补）：</p>
<figure><img src="https://pica.zhimg.com/v2-8448e03dc748fad9ecf3f39126dda558.jpg?source=8673f162" alt="示例图片"/><figcaption>图片加载示例</figcaption></figure>
<p>末尾再放一段文字，确认滚动区域可以一直滑到底部。</p>
"#;

#[cfg(any(target_os = "android", target_os = "ios"))]
fn safe_area() -> (f32, f32, f32, f32) {
    gpui_mobile::safe_area_insets()
}

/// How long the cold-start poster stays up before fading into the home page.
const SPLASH_DURATION: std::time::Duration = std::time::Duration::from_secs(5);
/// Fade length, shared by the natural end and the skip button.
const SPLASH_FADE: std::time::Duration = std::time::Duration::from_millis(200);

/// Whole seconds left on the splash clock, rounded up so the pill reads
/// "跳过 5" on the first frame and "跳过 1" just before the fade.
fn splash_remaining_secs(elapsed: std::time::Duration) -> u32 {
    let left = SPLASH_DURATION.saturating_sub(elapsed);
    let secs = left.as_secs() as u32 + u32::from(left.subsec_nanos() > 0);
    secs.max(1)
}

/// Offline fallback quotes when no poster has been cached yet.
const SPLASH_QUOTES: [(&str, &str); 5] = [
    (
        "生活不是等待暴风雨过去，而是学会在雨中跳舞。",
        "— 维维安·格林",
    ),
    ("我们都在阴沟里，但仍有人仰望星空。", "— 奥斯卡·王尔德"),
    (
        "世界上只有一种真正的英雄主义，就是认清生活的真相后依然热爱生活。",
        "— 罗曼·罗兰",
    ),
    ("纸上得来终觉浅，绝知此事要躬行。", "— 陆游"),
    ("路漫漫其修远兮，吾将上下而求索。", "— 屈原"),
];

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn safe_area() -> (f32, f32, f32, f32) {
    (0.0, 0.0, 0.0, 0.0)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
static PENDING_DEEPLINK: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(any(target_os = "android", target_os = "ios"))]
fn install_deeplink_handler() {
    gpui_mobile::packages::deeplink::set_deep_link_handler(|url| {
        if let Ok(mut slot) = PENDING_DEEPLINK.lock() {
            *slot = Some(url.to_string());
        }
    });
    if let Ok(Some(url)) = gpui_mobile::packages::deeplink::get_initial_link() {
        if let Ok(mut slot) = PENDING_DEEPLINK.lock() {
            *slot = Some(url);
        }
    }
}

pub(crate) fn open_external(url: &str) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = gpui_mobile::packages::url_launcher::launch_url(url);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }
}

fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ZHIHU_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        if let Ok(dir) = gpui_mobile::packages::path_provider::support_directory() {
            return dir.join("zhihu-rust");
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if let Some(dir) = dirs::data_dir() {
            return dir.join("zhihu-rust");
        }
    }
    std::env::temp_dir().join("zhihu-rust")
}

pub struct ZhihuApp {
    dark: bool,
    tab: usize,
    items: Vec<FeedItem>,
    feed: Vec<FeedItem>,
    feed_raw: Vec<FeedItem>,
    feed_page: u32,
    feed_end: bool,
    feed_loading_more: bool,
    follow_tab: FollowFeedKind,
    follow_feeds: [FollowFeedState; 2],
    feed_scroll: ScrollHandle,
    gesture_pending: bool,
    gesture_start_x: f32,
    pull_active: bool,
    pull_start_y: f32,
    pull_offset: f32,
    card_swipe_id: Option<String>,
    card_swipe_start_x: f32,
    card_swipe_dx: f32,
    card_swipe_open: Option<String>,
    card_swipe_suppress_until: Option<std::time::Instant>,
    sidebar_browse_open: bool,
    sidebar_my_open: bool,
    sidebar_collapsed: bool,
    recommend_mode: RecommendMode,
    recommend_preset: RecommendPreset,
    recommend_feedback_count: usize,
    /// Pool stats shown in the profile page; refreshed after collection.
    recommend_pool: RecommendPoolStats,
    /// Cached embedding rows for the bundled model.
    recommend_vectors: usize,
    recommend_model: String,
    recommend_embedding_fallback: bool,
    /// Ready vector profile, filled by the background embedding pass.
    vector_profile: Option<VectorProfile>,
    vectors_loading: bool,
    /// Pool changed while a pass was running; discard its result.
    vectors_dirty: bool,
    recommend_collecting: bool,
    recommend_last_report: Option<CollectReport>,
    recommend_cancel: Option<CancelToken>,
    /// Which ranking produced the visible feed.
    feed_source: FeedSource,
    loading_feed: bool,
    search: Entity<InputState>,
    cookie_input: Entity<InputState>,
    login_open: bool,
    login_status: Option<String>,
    account: Option<AccountProfile>,
    login_invalid: bool,
    qr_matrix: Option<Vec<Vec<bool>>>,
    qr_status: Option<String>,
    qr_risk: Option<(String, Option<String>)>,
    /// Cookies captured when the QR session hit risk control.
    qr_risk_cookies: Vec<(String, String)>,
    phone_input: Entity<InputState>,
    sms_input: Entity<InputState>,
    captcha_input: Entity<InputState>,
    phone_status: Option<String>,
    phone_captcha: Option<PathBuf>,
    phone_captcha_required: bool,
    image_paths: HashMap<String, PathBuf>,
    image_errors: HashSet<String>,
    services: Arc<AppServices>,
    hot: Vec<HotSearchItem>,
    daily: Vec<DailyStory>,
    loading_hot: bool,
    loading_daily: bool,
    daily_loading_more: bool,
    daily_end: bool,
    hot_requested: bool,
    daily_requested: bool,
    error: Option<String>,
    hot_error: Option<String>,
    daily_error: Option<String>,
    feed_error: Option<String>,
    focus_handle: FocusHandle,
    stack: Vec<Route>,
    detail: Option<ContentDetail>,
    detail_blocks: Vec<Block>,
    detail_scroll: ScrollHandle,
    detail_restore_progress: Option<f32>,
    detail_progress_saved: f32,
    detail_progress_saved_at: Option<std::time::Instant>,
    detail_loading: bool,
    detail_error: Option<String>,
    detail_offline: bool,
    detail_menu_open: bool,
    detail_selection_for: Option<String>,
    detail_highlight: Option<zhihu_types::Highlight>,
    pending_highlight_scroll: u8,
    speaking: bool,
    video_url: Option<String>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    video_player: Option<gpui_mobile::packages::video_player::VideoPlayer>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    video_handle: Option<Arc<gpui_mobile::platform_view::PlatformViewHandle>>,
    video_error: Option<String>,
    image_view: Option<PathBuf>,
    image_view_list: Vec<PathBuf>,
    image_zoom: f32,
    image_scroll: ScrollHandle,
    image_last_tap: Option<std::time::Instant>,
    /// Last two-finger pinch, used to suppress the pan-scroll it also produces.
    image_pinch_at: Option<std::time::Instant>,
    viewport_height: f32,
    image_view_index: usize,
    detail_vote: String,
    detail_following: bool,
    action_busy: bool,
    action_error: Option<String>,
    search_suggestions: Vec<String>,
    search_history: Vec<String>,
    search_results: Vec<FeedItem>,
    search_loading: bool,
    search_error: Option<String>,
    search_results_error: Option<String>,
    search_offset: u32,
    search_end: bool,
    search_loading_more: bool,
    search_vertical: SearchVertical,
    search_sort: SearchSort,
    blocklist_open: bool,
    blocklist_input: Entity<InputState>,
    blocklist: Vec<(String, bool)>,
    blocked_users: Vec<(String, String)>,
    blocked_topics: Vec<(String, String)>,
    topic_input: Entity<InputState>,
    blocklist_status: Option<String>,
    comments: Vec<zhihu_types::Comment>,
    comment_sort: CommentSort,
    /// Loaded comment pages by `kind:id:sort`, so re-entering a page shows the
    /// same list without another network round trip. Cleared on refresh.
    comments_cache: HashMap<String, CommentsCacheEntry>,
    /// Feed items the reader has opened (keyed `kind:id`), for gray titles.
    read_ids: HashSet<String>,
    /// When each feed card first appeared, so the mobile timeline is a stable
    /// descending sequence: newest pulled in first, older loaded further down.
    feed_times: HashMap<String, i64>,
    /// (kind, id, title, opened at) waiting out the 1 s read dwell.
    pending_read: Option<(ContentKind, String, String, std::time::Instant)>,
    comment_votes: HashMap<String, i8>,
    comment_likes: HashSet<String>,
    comment_action_error: Option<String>,
    question: Option<zhihu_types::QuestionDetail>,
    question_blocks: Vec<Block>,
    question_loading: bool,
    question_error: Option<String>,
    question_answers: Vec<FeedItem>,
    question_offset: u32,
    question_end: bool,
    question_loading_more: bool,
    question_answers_error: Option<String>,
    question_sort: QuestionSort,
    question_comments: Vec<zhihu_types::Comment>,
    question_comments_loading: bool,
    question_comments_error: Option<String>,
    question_comments_offset: u32,
    question_comments_end: bool,
    question_comments_loading_more: bool,
    topic: Option<TopicDetail>,
    topic_loading: bool,
    topic_error: Option<String>,
    topic_following: bool,
    topic_tab: TopicFeedTab,
    topic_sort: TopicSort,
    topic_items: Vec<FeedItem>,
    topic_next: Option<String>,
    topic_end: bool,
    topic_loading_more: bool,
    topic_feed_error: Option<String>,
    topic_scroll: ScrollHandle,
    topic_generation: u64,
    member: Option<zhihu_types::MemberProfile>,
    member_loading: bool,
    member_error: Option<String>,
    member_answers: Vec<FeedItem>,
    member_answers_offset: u32,
    member_answers_end: bool,
    member_answers_loading: bool,
    member_answers_error: Option<String>,
    member_following: bool,
    member_blocking: bool,
    member_search_open: bool,
    member_search_input: Entity<InputState>,
    hidden_recommend_users: Vec<(String, String)>,
    member_tab: MemberTab,
    member_collections: Vec<zhihu_types::Collection>,
    member_collections_offset: u32,
    member_collections_end: bool,
    member_collections_loading: bool,
    member_collections_error: Option<String>,
    member_columns: Vec<zhihu_types::MemberColumn>,
    member_columns_offset: u32,
    member_columns_end: bool,
    member_columns_loading: bool,
    member_columns_error: Option<String>,
    member_people: Vec<zhihu_types::MemberProfile>,
    member_people_offset: u32,
    member_people_end: bool,
    member_people_loading: bool,
    member_people_error: Option<String>,
    collections: Vec<zhihu_types::Collection>,
    collections_loading: bool,
    collections_error: Option<String>,
    collections_offset: u32,
    collections_end: bool,
    collection_items: Vec<FeedItem>,
    collection_items_loading: bool,
    collection_items_error: Option<String>,
    collection_items_offset: u32,
    collection_items_end: bool,
    content_collections: Vec<zhihu_types::Collection>,
    content_collections_loading: bool,
    content_collections_error: Option<String>,
    favorite_busy: bool,
    collection_title_input: Entity<InputState>,
    tts_rate: f32,
    tts_pitch: f32,
    tts_volume: f32,
    tts_voice: String,
    tts_voice_input: Entity<InputState>,
    tts_active: bool,
    tts_playing: bool,
    tts_segments: Vec<String>,
    tts_index: usize,
    tts_segment_started: std::time::Instant,
    tts_scroll: UniformListScrollHandle,
    toast_message: Option<String>,
    toast_until: Option<std::time::Instant>,
    comments_loading: bool,
    comments_error: Option<String>,
    comments_offset: u32,
    comments_end: bool,
    comments_loading_more: bool,
    comment_children: HashMap<String, Vec<zhihu_types::Comment>>,
    comment_children_end: HashMap<String, bool>,
    comment_children_open: HashSet<String>,
    comment_children_loading: HashSet<String>,
    comment_children_error: HashMap<String, String>,
    comment_anchor_root: Option<zhihu_types::Comment>,
    comment_anchor_target: Option<zhihu_types::Comment>,
    comment_highlight_id: Option<String>,
    comment_input: Entity<InputState>,
    comment_reply_target: Option<CommentReplyTarget>,
    comment_sending: bool,
    comment_send_error: Option<String>,
    comment_clear_pending: bool,
    notifications: Vec<zhihu_types::NotificationEntry>,
    notifications_loading: bool,
    notifications_error: Option<String>,
    notifications_offset: u32,
    notifications_end: bool,
    notifications_loading_more: bool,
    notifications_status: Option<String>,
    notification_unread: u64,
    notification_polled_at: Option<std::time::Instant>,
    history: Vec<zhihu_types::HistoryEntry>,
    highlights: Vec<zhihu_types::Highlight>,
    notes_error: Option<String>,
    selection_quote: Option<String>,
    selection_note_input: Entity<InputState>,
    selection_tags_input: Entity<InputState>,
    selection_color: String,
    crash_logs: Vec<crash::CrashLog>,
    crash_view: Option<PathBuf>,
    crash_text: Option<String>,
    crash_status: Option<String>,
    diag: Option<DiagSnapshot>,
    started_at: std::time::Instant,
    feed_loaded_ms: Option<u128>,
    detail_loaded_ms: Option<u128>,
    detail_requested_at: Option<std::time::Instant>,
    font_size: f32,
    line_height: f32,
    typography_preset: TypographyPreset,
    reading_indent: bool,
    pos_colors: bool,
    hide_mcn: bool,
    swipe_accum: f32,
    swipe_vertical: f32,
    /// The current gesture is locked to the horizontal axis and swallowed.
    swipe_decided: bool,
    swipe_gesture_active: bool,
    /// Live drag progress in [-1, 1]; the page follows the finger.
    swipe_preview: f32,
    list_epoch: u64,
    swipe_last_switch: Option<std::time::Instant>,
    /// Logical width of the content area, used for the 30% swipe threshold.
    viewport_width: f32,
    /// Monotonic id for the committed tab slide, and where it starts (a
    /// signed fraction of the viewport width).
    slide_seq: u64,
    slide_from: f32,
    /// Snap-back animation after a drag that stayed under the threshold.
    snap_seq: u64,
    snap_from: Option<f32>,
    /// Route push/pop slide animation (right side pushes, left resumes).
    route_anim_seq: u64,
    route_anim_from: f32,
    route_depth_seen: usize,
    /// 我的 page accordion groups (账号/内容/推荐/阅读/工具).
    profile_expanded: [bool; 5],
    /// Keeps the window touch-selection observer alive.
    touch_selection_watch: Option<gpui_kit::Subscription>,
    touch_selection_dirty: bool,
    hot_rotate_index: usize,
    hot_rotate_at: Option<std::time::Instant>,
    /// Cold-start poster overlay (image + quote) and its fade-out.
    splash_image: Option<PathBuf>,
    splash_quote: String,
    splash_source: String,
    splash_visible: bool,
    splash_hiding: bool,
    splash_armed: bool,
    splash_fade_seq: u64,
    splash_started_at: std::time::Instant,
    splash_hide_at: Option<std::time::Instant>,
    /// Last countdown second painted, so the tick only repaints on changes.
    splash_shown_sec: u32,
}

impl ZhihuApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        std::thread::Builder::new()
            .name("zhihu-pos-warmup".to_string())
            .spawn(pos::warm_up)
            .ok();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索知乎"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let query = this.search.read(cx).value().trim().to_string();
                if !query.is_empty() {
                    this.submit_search_query(query, cx);
                    cx.notify();
                }
            }
        })
        .detach();
        let cookie_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("粘贴浏览器 Cookie，例如 z_c0=...; d_c0=...;")
        });
        let blocklist_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入要屏蔽的关键词"));
        let topic_input = cx.new(|cx| InputState::new(window, cx).placeholder("输入要屏蔽的话题"));
        let collection_title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("新收藏夹名称"));
        let comment_input = cx.new(|cx| InputState::new(window, cx).placeholder("写评论…"));
        let member_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索 TA 的创作"));
        let selection_note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("写下你的想法…"));
        let selection_tags_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("标签，用逗号分隔"));
        cx.subscribe(&comment_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_comment(cx);
            }
        })
        .detach();
        let phone_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("手机号，例如 13800138000"));
        let sms_input = cx.new(|cx| InputState::new(window, cx).placeholder("6 位短信验证码"));
        let captcha_input = cx.new(|cx| InputState::new(window, cx).placeholder("图形验证码"));
        let services =
            Arc::new(AppServices::new(data_dir()).expect("failed to start network runtime"));
        let cached_splash = services.cached_splash();
        services.refresh_splash();
        let read_ids = services.read_ids();
        let receiver = services.receiver();
        let logged_in = services.is_logged_in();
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let Some(this) = this.upgrade() else {
                    break;
                };
                this.update(cx, move |this, cx| {
                    this.on_event(event);
                    cx.notify();
                });
            }
        })
        .detach();

        // Feed pagination is driven by a light timer: GPUI's scroll handle is
        // shared, so checking it here avoids instrumenting every wheel event.
        //
        // The interval adapts: 250 ms while something is animating, loading or
        // playing (the splash countdown needs it too), otherwise 1 s. An idle
        // app used to wake the looper four times a second forever, which is
        // wasted battery on Android.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        install_deeplink_handler();
        cx.spawn(async move |this, cx| loop {
            let busy = this
                .upgrade()
                .map(|this| this.update(cx, |this, _| this.ui_busy()))
                .unwrap_or(false);
            let interval = if busy { 250 } else { 1_000 };
            cx.background_executor()
                .timer(std::time::Duration::from_millis(interval))
                .await;
            let Some(this) = this.upgrade() else {
                break;
            };
            this.update(cx, |this, cx| {
                this.maybe_load_more_feed(cx);
                this.tick_toast(cx);
                this.tick_tts(cx);
                this.tick_detail_progress();
                this.poll_notification_unread();
                this.tick_hot_rotation(cx);
                this.tick_splash(cx);
                // Warm the public tabs once the splash is fading; the call is
                // one-shot and the feed is only touched with a session.
                if this.started_at.elapsed() >= std::time::Duration::from_millis(1200) {
                    this.services.prefetch_startup();
                }
                this.tick_pending_read();
                this.poll_imported_login(cx);
                this.poll_deeplink(cx);
            });
        })
        .detach();

        services.refresh_hot_search();
        services.refresh_daily();
        if logged_in {
            services.validate_login();
            services.refresh_feed();
        }

        // Dev aid: ZHIHU_TAB=0..3 selects the initial tab.
        let blocklist = services.block_keywords();
        let blocked_users = services.blocked_users();
        let blocked_topics = services.blocked_topics();
        let settings = services.settings();
        let initial_search_history = services.search_history(8);
        let recommend_feedback_count = services.recommend_feedback_count();
        let initial_hidden_recommend_users = services.recommend_hidden_users();
        let initial_highlights = services.highlights(500);
        let items = filter_sample_items(
            m0_sample_feed(),
            &blocklist,
            &blocked_users,
            &blocked_topics,
        );
        let tab = std::env::var("ZHIHU_TAB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|index| *index < TABS.len())
            .unwrap_or(settings.tab);

        Self {
            dark: settings.dark,
            tab,
            items,
            feed: Vec::new(),
            feed_raw: Vec::new(),
            feed_page: 0,
            feed_end: false,
            feed_loading_more: false,
            follow_tab: FollowFeedKind::Recommend,
            follow_feeds: Default::default(),
            feed_scroll: ScrollHandle::new(),
            gesture_pending: false,
            gesture_start_x: 0.0,
            pull_active: false,
            pull_start_y: 0.0,
            pull_offset: 0.0,
            card_swipe_id: None,
            card_swipe_start_x: 0.0,
            card_swipe_dx: 0.0,
            card_swipe_open: None,
            card_swipe_suppress_until: None,
            sidebar_browse_open: true,
            sidebar_my_open: true,
            sidebar_collapsed: false,
            recommend_mode: settings.recommend_mode,
            recommend_preset: settings.recommend_preset,
            recommend_feedback_count,
            recommend_pool: services.recommend_pool_stats(),
            recommend_vectors: services.recommend_embedding_count(),
            recommend_model: services.embedding_model_id().to_string(),
            recommend_embedding_fallback: services.embedding_fallback(),
            vector_profile: None,
            vectors_loading: false,
            vectors_dirty: false,
            recommend_collecting: false,
            recommend_last_report: None,
            recommend_cancel: None,
            feed_source: FeedSource::Server,
            loading_feed: logged_in,
            search,
            cookie_input,
            login_open: false,
            login_status: None,
            account: None,
            login_invalid: false,
            qr_matrix: None,
            qr_status: None,
            qr_risk: None,
            qr_risk_cookies: Vec::new(),
            phone_input,
            sms_input,
            captcha_input,
            phone_status: None,
            phone_captcha: None,
            phone_captcha_required: false,
            image_paths: HashMap::new(),
            image_errors: HashSet::new(),
            services,
            hot: Vec::new(),
            daily: Vec::new(),
            loading_hot: true,
            loading_daily: true,
            daily_loading_more: false,
            daily_end: false,
            hot_requested: true,
            daily_requested: true,
            error: None,
            hot_error: None,
            daily_error: None,
            feed_error: None,
            focus_handle,
            stack: Vec::new(),
            detail: None,
            detail_blocks: Vec::new(),
            detail_scroll: ScrollHandle::new(),
            detail_restore_progress: None,
            detail_progress_saved: 0.0,
            detail_progress_saved_at: None,
            detail_loading: false,
            detail_error: None,
            detail_offline: false,
            detail_menu_open: false,
            detail_selection_for: None,
            detail_highlight: None,
            pending_highlight_scroll: 0,
            speaking: false,
            video_url: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            video_player: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            video_handle: None,
            video_error: None,
            image_view: None,
            image_view_list: Vec::new(),
            image_zoom: 1.0,
            image_scroll: ScrollHandle::new(),
            image_last_tap: None,
            image_pinch_at: None,
            viewport_height: 800.0,
            image_view_index: 0,
            detail_vote: "neutral".to_string(),
            detail_following: false,
            action_busy: false,
            action_error: None,
            search_suggestions: Vec::new(),
            search_history: initial_search_history,
            search_results: Vec::new(),
            search_loading: false,
            search_error: None,
            search_results_error: None,
            search_offset: 0,
            search_end: false,
            search_loading_more: false,
            search_vertical: SearchVertical::All,
            search_sort: SearchSort::Default,
            blocklist_open: false,
            blocklist_input,
            blocklist,
            blocked_users,
            blocked_topics,
            topic_input,
            blocklist_status: None,
            comments: Vec::new(),
            comment_sort: CommentSort::Default,
            comments_cache: HashMap::new(),
            read_ids,
            feed_times: HashMap::new(),
            pending_read: None,
            comment_votes: HashMap::new(),
            comment_likes: HashSet::new(),
            comment_action_error: None,
            question: None,
            question_blocks: Vec::new(),
            question_loading: false,
            question_error: None,
            question_answers: Vec::new(),
            question_offset: 0,
            question_end: false,
            question_loading_more: false,
            question_answers_error: None,
            question_sort: QuestionSort::Default,
            question_comments: Vec::new(),
            question_comments_loading: false,
            question_comments_error: None,
            question_comments_offset: 0,
            question_comments_end: false,
            question_comments_loading_more: false,
            topic: None,
            topic_loading: false,
            topic_error: None,
            topic_following: false,
            topic_tab: TopicFeedTab::Discussion,
            topic_sort: TopicSort::Essence,
            topic_items: Vec::new(),
            topic_next: None,
            topic_end: false,
            topic_loading_more: false,
            topic_feed_error: None,
            topic_scroll: ScrollHandle::new(),
            topic_generation: 0,
            member: None,
            member_loading: false,
            member_error: None,
            member_answers: Vec::new(),
            member_answers_offset: 0,
            member_answers_end: false,
            member_answers_loading: false,
            member_answers_error: None,
            member_following: false,
            member_blocking: false,
            member_search_open: false,
            member_search_input,
            hidden_recommend_users: initial_hidden_recommend_users,
            member_tab: MemberTab::Answers,
            member_collections: Vec::new(),
            member_collections_offset: 0,
            member_collections_end: false,
            member_collections_loading: false,
            member_collections_error: None,
            member_columns: Vec::new(),
            member_columns_offset: 0,
            member_columns_end: false,
            member_columns_loading: false,
            member_columns_error: None,
            member_people: Vec::new(),
            member_people_offset: 0,
            member_people_end: false,
            member_people_loading: false,
            member_people_error: None,
            collections: Vec::new(),
            collections_loading: false,
            collections_error: None,
            collections_offset: 0,
            collections_end: false,
            collection_items: Vec::new(),
            collection_items_loading: false,
            collection_items_error: None,
            collection_items_offset: 0,
            collection_items_end: false,
            content_collections: Vec::new(),
            content_collections_loading: false,
            content_collections_error: None,
            favorite_busy: false,
            collection_title_input,
            tts_rate: settings.tts_rate,
            tts_pitch: settings.tts_pitch,
            tts_volume: settings.tts_volume,
            tts_voice: settings.tts_voice.clone(),
            tts_voice_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("系统音色名称，留空使用默认")
                    .default_value(settings.tts_voice)
            }),
            tts_active: false,
            tts_playing: false,
            tts_segments: Vec::new(),
            tts_index: 0,
            tts_segment_started: std::time::Instant::now(),
            tts_scroll: UniformListScrollHandle::new(),
            toast_message: None,
            toast_until: None,
            comments_loading: false,
            comments_error: None,
            comments_offset: 0,
            comments_end: false,
            comments_loading_more: false,
            comment_children: HashMap::new(),
            comment_children_end: HashMap::new(),
            comment_children_open: HashSet::new(),
            comment_children_loading: HashSet::new(),
            comment_children_error: HashMap::new(),
            comment_anchor_root: None,
            comment_anchor_target: None,
            comment_highlight_id: None,
            comment_input,
            comment_reply_target: None,
            comment_sending: false,
            comment_send_error: None,
            comment_clear_pending: false,
            notifications: Vec::new(),
            notifications_loading: false,
            notifications_error: None,
            notifications_offset: 0,
            notifications_end: false,
            notifications_loading_more: false,
            notifications_status: None,
            notification_unread: 0,
            notification_polled_at: None,
            history: Vec::new(),
            highlights: initial_highlights,
            notes_error: None,
            selection_quote: None,
            selection_note_input,
            selection_tags_input,
            selection_color: "yellow".to_string(),
            crash_logs: crash::list_crashes(),
            crash_view: None,
            crash_text: None,
            crash_status: None,
            diag: None,
            started_at: std::time::Instant::now(),
            feed_loaded_ms: None,
            detail_loaded_ms: None,
            detail_requested_at: None,
            font_size: settings.font_size,
            line_height: settings.line_height,
            typography_preset: TypographyPreset::parse(&settings.typography),
            reading_indent: settings.reading_indent,
            pos_colors: settings.pos_colors,
            hide_mcn: settings.hide_mcn,
            swipe_accum: 0.0,
            swipe_vertical: 0.0,
            swipe_decided: false,
            swipe_gesture_active: false,
            swipe_preview: 0.0,
            list_epoch: 0,
            swipe_last_switch: None,
            viewport_width: 400.0,
            slide_seq: 0,
            slide_from: 0.0,
            snap_seq: 0,
            snap_from: None,
            route_anim_seq: 0,
            route_anim_from: 0.0,
            route_depth_seen: 0,
            profile_expanded: [false; 5],
            touch_selection_watch: None,
            touch_selection_dirty: false,
            hot_rotate_index: 0,
            hot_rotate_at: None,
            splash_visible: true,
            splash_hiding: false,
            splash_armed: false,
            splash_fade_seq: 0,
            splash_started_at: std::time::Instant::now(),
            splash_hide_at: None,
            splash_shown_sec: 0,
            splash_image: cached_splash.as_ref().map(|card| card.image_path.clone()),
            splash_quote: cached_splash
                .as_ref()
                .map(|card| card.quote.clone())
                .unwrap_or_default(),
            splash_source: cached_splash.map(|card| card.source).unwrap_or_default(),
        }
    }

    fn on_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::HotSearch(items) => {
                self.hot = items;
                self.hot_error = None;
                self.loading_hot = false;
            }
            UiEvent::Daily(items) => {
                self.daily_end = items.is_empty();
                self.daily = items;
                self.daily_loading_more = false;
                self.daily_error = None;
                self.loading_daily = false;
            }
            UiEvent::DailyMore(items) => {
                if items.is_empty() {
                    self.daily_end = true;
                } else {
                    let date = self.daily.last().map(|story| story.date.clone());
                    let mut known: HashSet<u64> = self.daily.iter().map(|story| story.id).collect();
                    for item in items {
                        if known.insert(item.id) {
                            self.daily.push(item);
                        }
                    }
                    if self.daily.last().map(|story| story.date.clone()) == date {
                        self.daily_end = true;
                    }
                }
                self.daily_loading_more = false;
                self.daily_error = None;
            }
            UiEvent::DailyMoreError(message) => {
                self.daily_loading_more = false;
                self.daily_error = Some(message);
            }
            UiEvent::DailyDetail(detail) => {
                if matches!(
                    self.stack.last(),
                    Some(Route::Daily { id, .. }) if *id == detail.id
                ) {
                    self.detail_blocks = zhihu_content::parse_html(&detail.body_html).blocks;
                    self.detail = Some(ContentDetail {
                        id: format!("daily-{}", detail.id),
                        title: detail.title,
                        author: Author {
                            id: "daily".to_string(),
                            name: "知乎日报".to_string(),
                            headline: Some("知乎日报".to_string()),
                            url_token: None,
                            is_following: false,
                            avatar_url: None,
                            is_mcn: false,
                        },
                        kind: ContentKind::Article,
                        content_html: detail.body_html,
                        excerpt: detail.hint,
                        vote_up_count: 0,
                        comment_count: 0,
                        vote_state: "neutral".to_string(),
                        created_time: 0,
                        updated_time: 0,
                        question_id: None,
                    });
                    self.detail_loading = false;
                    self.detail_error = None;
                }
            }
            UiEvent::DailyDetailError(message) => {
                self.detail_loading = false;
                self.detail_error = Some(message);
            }
            UiEvent::Feed { items, is_end } => {
                if self.feed_loaded_ms.is_none() {
                    self.feed_loaded_ms = Some(self.started_at.elapsed().as_millis());
                }
                self.feed_end = is_end;
                self.feed_page = 1;
                self.feed_raw = items;
                self.apply_recommendation();
                stamp_feed_times(&mut self.feed_times, &self.feed_raw, true);
                self.loading_feed = false;
                self.feed_loading_more = false;
                self.feed_error = None;
                self.error = None;
            }
            UiEvent::FeedOffline { items } => {
                if self.feed_loaded_ms.is_none() {
                    self.feed_loaded_ms = Some(self.started_at.elapsed().as_millis());
                }
                self.feed_end = true;
                self.feed_page = 1;
                self.feed_raw = items;
                self.apply_recommendation();
                stamp_feed_times(&mut self.feed_times, &self.feed_raw, true);
                self.loading_feed = false;
                self.feed_loading_more = false;
                self.feed_error = Some("网络不可用，正在显示离线缓存的推荐".to_string());
                self.error = None;
            }
            UiEvent::FeedCached(items) => {
                // Snapshot on screen while the network refresh runs; no error
                // banner and pagination stays available.
                if self.feed_loaded_ms.is_none() {
                    self.feed_loaded_ms = Some(self.started_at.elapsed().as_millis());
                }
                self.feed_page = 1;
                self.feed_raw = items;
                self.apply_recommendation();
                stamp_feed_times(&mut self.feed_times, &self.feed_raw, true);
                self.loading_feed = false;
                self.feed_error = None;
            }
            UiEvent::FeedMore { items, is_end } => {
                let empty = items.is_empty();
                stamp_feed_times(&mut self.feed_times, &items, false);
                let mut known: HashSet<String> =
                    self.feed_raw.iter().map(|item| item.id.clone()).collect();
                for item in items {
                    if known.insert(item.id.clone()) {
                        self.feed_raw.push(item);
                    }
                }
                self.feed_page = self.feed_page.saturating_add(1);
                self.feed_end = is_end || empty;
                self.feed_loading_more = false;
                self.apply_recommendation();
                self.feed_error = None;
            }
            UiEvent::FeedMoreError(message) => {
                self.feed_loading_more = false;
                self.feed_error = Some(message);
            }
            UiEvent::FollowFeed {
                kind,
                items,
                append,
                is_end,
                next,
            } => {
                self.follow_feeds[follow_index(kind)].apply_page(items, append, is_end, next);
            }
            UiEvent::FollowFeedError { kind, message } => {
                let state = &mut self.follow_feeds[follow_index(kind)];
                state.loading = false;
                state.loading_more = false;
                state.error = Some(message);
            }
            UiEvent::Detail(detail) => {
                self.accept_detail(detail, false);
            }
            UiEvent::DetailOffline(detail) => {
                self.accept_detail(detail, true);
            }
            UiEvent::DetailError(message) => {
                self.detail_loading = false;
                self.detail_error = Some(message);
            }
            UiEvent::Question(question) => {
                self.question_blocks = zhihu_content::parse_html(&question.detail_html).blocks;
                self.question = Some(question);
                self.question_loading = false;
                self.question_error = None;
            }
            UiEvent::QuestionError(message) => {
                self.question_loading = false;
                self.question_error = Some(message);
            }
            UiEvent::QuestionAnswers {
                items,
                append,
                is_end,
            } => {
                if append {
                    self.question_answers.extend(items);
                } else {
                    self.question_answers = items;
                }
                self.question_offset = self.question_answers.len() as u32;
                self.question_end = is_end;
                self.question_loading_more = false;
                self.question_answers_error = None;
            }
            UiEvent::QuestionAnswersError(message) => {
                self.question_loading_more = false;
                self.question_answers_error = Some(message);
            }
            UiEvent::Topic(topic) => {
                if matches!(
                    self.stack.last(),
                    Some(Route::Topic { id, .. }) if *id == topic.id
                ) {
                    if let Some(Route::Topic { name, .. }) = self.stack.last_mut() {
                        *name = topic.name.clone();
                    }
                    self.topic_following = topic.is_following;
                    self.topic = Some(topic);
                    self.topic_loading = false;
                    self.topic_error = None;
                }
            }
            UiEvent::TopicError(message) => {
                self.topic_loading = false;
                self.topic_error = Some(message);
            }
            UiEvent::TopicFeed {
                topic_id,
                generation,
                items,
                append,
                is_end,
                next,
            } => {
                let current = matches!(
                    self.stack.last(),
                    Some(Route::Topic { id, .. }) if *id == topic_id
                );
                if !current || generation != self.topic_generation {
                    return;
                }
                if append {
                    let mut known: HashSet<String> = self
                        .topic_items
                        .iter()
                        .map(|item| item.id.clone())
                        .collect();
                    for item in items {
                        if known.insert(item.id.clone()) {
                            self.topic_items.push(item);
                        }
                    }
                } else {
                    self.topic_items = items;
                }
                self.topic_next = next;
                self.topic_end = is_end;
                self.topic_loading_more = false;
                self.topic_feed_error = None;
            }
            UiEvent::TopicFeedError {
                topic_id,
                generation,
                message,
            } => {
                let current = matches!(
                    self.stack.last(),
                    Some(Route::Topic { id, .. }) if *id == topic_id
                );
                if current && generation == self.topic_generation {
                    self.topic_loading_more = false;
                    self.topic_feed_error = Some(message);
                }
            }
            UiEvent::TopicFollowed(following) => {
                self.topic_following = following;
                if let Some(topic) = &mut self.topic {
                    topic.is_following = following;
                }
                self.topic_error = None;
            }
            UiEvent::TopicFollowError(message) => {
                self.topic_error = Some(message);
            }
            UiEvent::QuestionFound { id, title } => {
                self.open_question(id, title);
            }
            UiEvent::QuestionNotFound(query) => {
                self.open_search(query);
            }
            UiEvent::Member(member) => {
                if let Some(Route::Member { name, .. }) = self.stack.last_mut() {
                    *name = member.name.clone();
                }
                self.member_following = member.is_following;
                self.member_blocking = member.is_blocking;
                self.member = Some(member);
                self.member_loading = false;
                self.member_error = None;
            }
            UiEvent::MemberError(message) => {
                self.member_loading = false;
                self.member_error = Some(message);
            }
            UiEvent::MemberAnswers {
                kind,
                items,
                append,
                is_end,
            } => {
                if self.member_tab.content_kind() != Some(kind) {
                    return;
                }
                if append {
                    self.member_answers.extend(items);
                } else {
                    self.member_answers = items;
                }
                self.member_answers_offset = self.member_answers.len() as u32;
                self.member_answers_end = is_end;
                self.member_answers_loading = false;
                self.member_answers_error = None;
            }
            UiEvent::MemberAnswersError(message) => {
                if self.member_tab.content_kind().is_none() {
                    return;
                }
                self.member_answers_loading = false;
                self.member_answers_error = Some(message);
            }
            UiEvent::MemberActivities {
                items,
                append,
                is_end,
            } => {
                if self.member_tab != MemberTab::Activities {
                    return;
                }
                if append {
                    self.member_answers.extend(items);
                } else {
                    self.member_answers = items;
                }
                self.member_answers_offset = self.member_answers.len() as u32;
                self.member_answers_end = is_end;
                self.member_answers_loading = false;
                self.member_answers_error = None;
            }
            UiEvent::MemberActivitiesError(message) => {
                if self.member_tab != MemberTab::Activities {
                    return;
                }
                self.member_answers_loading = false;
                self.member_answers_error = Some(message);
            }
            UiEvent::MemberCollections {
                items,
                append,
                is_end,
            } => {
                if self.member_tab != MemberTab::Collections {
                    return;
                }
                if append {
                    self.member_collections.extend(items);
                } else {
                    self.member_collections = items;
                }
                self.member_collections_offset = self.member_collections.len() as u32;
                self.member_collections_end = is_end;
                self.member_collections_loading = false;
                self.member_collections_error = None;
            }
            UiEvent::MemberCollectionsError(message) => {
                self.member_collections_loading = false;
                self.member_collections_error = Some(message);
            }
            UiEvent::MemberColumns {
                items,
                append,
                is_end,
            } => {
                if self.member_tab != MemberTab::Columns {
                    return;
                }
                if append {
                    self.member_columns.extend(items);
                } else {
                    self.member_columns = items;
                }
                self.member_columns_offset = self.member_columns.len() as u32;
                self.member_columns_end = is_end;
                self.member_columns_loading = false;
                self.member_columns_error = None;
            }
            UiEvent::MemberColumnsError(message) => {
                self.member_columns_loading = false;
                self.member_columns_error = Some(message);
            }
            UiEvent::MemberPeople {
                following,
                items,
                append,
                is_end,
            } => {
                if self.member_tab.following() != Some(following) {
                    return;
                }
                if append {
                    self.member_people.extend(items);
                } else {
                    self.member_people = items;
                }
                self.member_people_offset = self.member_people.len() as u32;
                self.member_people_end = is_end;
                self.member_people_loading = false;
                self.member_people_error = None;
            }
            UiEvent::MemberPeopleError(message) => {
                self.member_people_loading = false;
                self.member_people_error = Some(message);
            }
            UiEvent::MemberFollowed(following) => {
                self.member_following = following;
                self.member_error = None;
            }
            UiEvent::MemberBlocked(blocking) => {
                self.member_blocking = blocking;
                self.member_error = None;
            }
            UiEvent::MemberBlockError(message) => {
                self.member_error = Some(message);
            }
            UiEvent::Collections {
                items,
                append,
                is_end,
            } => {
                if append {
                    self.collections.extend(items);
                } else {
                    self.collections = items;
                }
                self.collections_offset = self.collections.len() as u32;
                self.collections_end = is_end;
                self.collections_loading = false;
                self.collections_error = None;
            }
            UiEvent::CollectionsError(message) => {
                self.collections_loading = false;
                self.collections_error = Some(message);
            }
            UiEvent::CollectionItems {
                items,
                append,
                is_end,
            } => {
                if append {
                    self.collection_items.extend(items);
                } else {
                    self.collection_items = items;
                }
                self.collection_items_offset = self.collection_items.len() as u32;
                self.collection_items_end = is_end;
                self.collection_items_loading = false;
                self.collection_items_error = None;
            }
            UiEvent::CollectionItemsError(message) => {
                self.collection_items_loading = false;
                self.collection_items_error = Some(message);
            }
            UiEvent::CollectionDetail(collection) => {
                if let Some(Route::Collection { title, .. }) = self.stack.last_mut() {
                    *title = collection.title;
                }
            }
            UiEvent::ContentCollections(items) => {
                self.content_collections = items;
                self.content_collections_loading = false;
                self.content_collections_error = None;
            }
            UiEvent::ContentCollectionsError(message) => {
                self.content_collections_loading = false;
                self.content_collections_error = Some(message);
            }
            UiEvent::FavoriteUpdated {
                collection_id,
                favorite,
            } => {
                if let Some(collection) = self
                    .content_collections
                    .iter_mut()
                    .find(|collection| collection.id == collection_id)
                {
                    collection.is_favorited = favorite;
                }
                self.favorite_busy = false;
                self.content_collections_error = None;
                if favorite {
                    self.record_detail_action("collect", 2.0);
                }
                self.toast_message = Some(if favorite {
                    "✓ 已收藏".to_string()
                } else {
                    "✓ 已取消收藏".to_string()
                });
                self.toast_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
            UiEvent::CollectionCreated(collection) => {
                self.favorite_busy = false;
                self.content_collections_error = None;
                if !self
                    .content_collections
                    .iter()
                    .any(|item| item.id == collection.id)
                {
                    self.content_collections.push(collection);
                }
                self.toast_message = Some("✓ 已新建收藏夹，点击即可收藏".to_string());
                self.toast_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
            UiEvent::CollectionCreateError(message) => {
                self.favorite_busy = false;
                self.content_collections_error = Some(message);
            }
            UiEvent::FavoriteError(message) => {
                self.favorite_busy = false;
                self.content_collections_error = Some(message);
            }
            UiEvent::CommentLikeUpdated { comment_id, liked } => {
                if liked {
                    self.comment_likes.insert(comment_id);
                } else {
                    self.comment_likes.remove(&comment_id);
                }
                self.comment_action_error = None;
            }
            UiEvent::CommentLikeError(message) => {
                self.comment_action_error = Some(message);
            }
            UiEvent::CommentSubmitted {
                target_kind,
                target_id,
                root_id,
                comment,
            } => {
                let current = matches!(
                    self.stack.last(),
                    Some(Route::Comments { kind, id, .. })
                        if *kind == target_kind && *id == target_id
                );
                if !current {
                    return;
                }
                if let Some(root_id) = root_id {
                    if let Some(root) = self.comments.iter_mut().find(|root| root.id == root_id) {
                        root.child_count = root.child_count.saturating_add(1);
                        let children = self
                            .comment_children
                            .entry(root_id.clone())
                            .or_insert_with(|| root.children.clone());
                        if !children.iter().any(|child| child.id == comment.id) {
                            children.push(comment.clone());
                        }
                        self.comment_children_open.insert(root_id.clone());
                        self.comment_children_loading.remove(&root_id);
                        self.comment_children_error.remove(&root_id);
                    }
                } else if !self.comments.iter().any(|root| root.id == comment.id) {
                    self.comments.insert(0, comment);
                }
                if let Some(detail) = &mut self.detail {
                    if detail.id == target_id {
                        detail.comment_count = detail.comment_count.saturating_add(1);
                    }
                }
                self.comment_sending = false;
                self.comment_send_error = None;
                self.comment_reply_target = None;
                self.comment_clear_pending = true;
            }
            UiEvent::CommentSubmitError(message) => {
                self.comment_sending = false;
                self.comment_send_error = Some(message);
            }
            UiEvent::CommentAnchor {
                root,
                target,
                is_child,
            } => {
                let current = matches!(
                    self.stack.last(),
                    Some(Route::Comments { anchor: Some(anchor), .. })
                        if *anchor == target.id
                );
                if !current {
                    return;
                }
                self.comment_highlight_id = Some(target.id.clone());
                self.comment_anchor_root = Some((*root).clone());
                self.comment_anchor_target = Some((*target).clone());
                self.comments.retain(|comment| comment.id != root.id);
                self.comments.insert(0, (*root).clone());
                if is_child {
                    let children = self.comment_children.entry(root.id.clone()).or_default();
                    if !children.iter().any(|comment| comment.id == target.id) {
                        children.insert(0, (*target).clone());
                    }
                    self.comment_children_open.insert(root.id.clone());
                    self.comment_children_loading.insert(root.id.clone());
                    self.services.load_child_comments(root.id, 0, false);
                }
                self.comment_action_error = None;
            }
            UiEvent::CommentAnchorError(message) => {
                self.comment_action_error = Some(message);
            }
            UiEvent::VoteUpdated(count) => {
                self.action_busy = false;
                self.action_error = None;
                if let Some(detail) = &mut self.detail {
                    detail.vote_up_count = count;
                }
            }
            UiEvent::FollowUpdated(following) => {
                self.action_busy = false;
                self.action_error = None;
                self.detail_following = following;
                if let Some(detail) = &mut self.detail {
                    detail.author.is_following = following;
                }
            }
            UiEvent::ActionError(message) => {
                self.action_busy = false;
                self.action_error = Some(message);
            }
            UiEvent::SearchSuggestions(items) => {
                self.search_suggestions = items;
                self.search_loading = false;
                self.search_error = None;
            }
            UiEvent::SearchError(message) => {
                self.search_loading = false;
                self.search_error = Some(message);
            }
            UiEvent::SearchResults {
                items,
                append,
                is_end,
            } => {
                if append {
                    self.search_results.extend(items);
                } else {
                    self.search_results = items;
                }
                self.search_offset = self.search_results.len() as u32;
                self.search_end = is_end;
                self.search_loading = false;
                self.search_loading_more = false;
                self.search_error = None;
                self.search_results_error = None;
            }
            UiEvent::SearchResultsError(message) => {
                self.search_loading = false;
                self.search_loading_more = false;
                self.search_results_error = Some(message);
            }
            UiEvent::CommentsPage {
                kind,
                target_id,
                items,
                append,
                is_end,
            } => {
                let is_question_page = kind == ContentKind::Question
                    && matches!(
                        self.stack.last(),
                        Some(Route::Question { id, .. }) if *id == target_id
                    );
                if is_question_page {
                    if append {
                        self.question_comments.extend(items);
                    } else {
                        self.question_comments = items;
                    }
                    self.question_comments_offset = self.question_comments.len() as u32;
                    self.question_comments_end = is_end;
                    self.question_comments_loading = false;
                    self.question_comments_loading_more = false;
                    self.question_comments_error = None;
                    return;
                }
                if !matches!(
                    self.stack.last(),
                    Some(Route::Comments { kind: current_kind, id, .. })
                        if *current_kind == kind && *id == target_id
                ) {
                    return;
                }
                if append {
                    self.comments.extend(items);
                } else {
                    self.comments = items;
                    if let Some(anchor_root) = self.comment_anchor_root.clone() {
                        if let Some(index) = self
                            .comments
                            .iter()
                            .position(|comment| comment.id == anchor_root.id)
                        {
                            let root = self.comments.remove(index);
                            self.comments.insert(0, root);
                        } else {
                            self.comments.insert(0, anchor_root);
                        }
                    }
                }
                self.comments_offset = self.comments.len() as u32;
                self.comments_end = is_end;
                self.comments_loading = false;
                self.comments_loading_more = false;
                self.comments_error = None;
                self.remember_comments();
            }
            UiEvent::Notifications {
                items,
                timeline_len,
                append,
                is_end,
                total_unread,
            } => {
                if append {
                    for item in items {
                        if !self
                            .notifications
                            .iter()
                            .any(|existing| existing.id == item.id)
                        {
                            self.notifications.push(item);
                        }
                    }
                    self.notifications_offset = self
                        .notifications_offset
                        .saturating_add(u32::try_from(timeline_len).unwrap_or(u32::MAX));
                } else {
                    self.notifications = items;
                    self.notifications_offset = u32::try_from(timeline_len).unwrap_or(u32::MAX);
                }
                self.notification_unread = total_unread;
                self.notifications_end = is_end;
                self.notifications_loading = false;
                self.notifications_loading_more = false;
                self.notifications_error = None;
            }
            UiEvent::NotificationsError(message) => {
                self.notifications_loading = false;
                self.notifications_loading_more = false;
                self.notifications_error = Some(message);
            }
            UiEvent::NotificationUnread(count) => {
                self.notification_unread = count;
            }
            UiEvent::NotificationsRead => {
                self.notification_unread = 0;
                self.notifications_status = Some("已同步已读状态".to_string());
            }
            UiEvent::NotificationsReadError(message) => {
                self.notifications_status = Some(message);
            }
            UiEvent::CommentsError {
                kind,
                target_id,
                message,
            } => {
                if kind == ContentKind::Question
                    && matches!(
                        self.stack.last(),
                        Some(Route::Question { id, .. }) if *id == target_id
                    )
                {
                    self.question_comments_loading = false;
                    self.question_comments_loading_more = false;
                    self.question_comments_error = Some(message);
                } else {
                    self.comments_loading = false;
                    self.comments_loading_more = false;
                    self.comments_error = Some(message);
                }
            }
            UiEvent::ChildComments {
                parent_id,
                items,
                append,
                is_end,
            } => {
                if append {
                    self.comment_children
                        .entry(parent_id.clone())
                        .or_default()
                        .extend(items);
                } else {
                    self.comment_children.insert(parent_id.clone(), items);
                }
                self.comment_children_end.insert(parent_id.clone(), is_end);
                self.comment_children_open.insert(parent_id.clone());
                self.comment_children_loading.remove(&parent_id);
                self.comment_children_error.remove(&parent_id);
                if self
                    .comment_anchor_root
                    .as_ref()
                    .is_some_and(|root| root.id == parent_id)
                {
                    if let Some(target) = self.comment_anchor_target.clone() {
                        let children = self.comment_children.entry(parent_id).or_default();
                        if !children.iter().any(|comment| comment.id == target.id) {
                            children.insert(0, target);
                        }
                    }
                }
            }
            UiEvent::ChildCommentsError { parent_id, message } => {
                self.comment_children_loading.remove(&parent_id);
                self.comment_children_error.insert(parent_id, message);
            }
            UiEvent::QrCode { link, .. } => {
                self.qr_risk = None;
                self.qr_matrix = qr_matrix_from_link(&link);
                self.qr_status = Some("请用知乎 App 扫码登录".to_string());
            }
            UiEvent::QrStatus(message) => {
                self.qr_status = Some(message);
            }
            UiEvent::ImageReady { url, path } => {
                self.image_errors.remove(&url);
                self.image_paths.insert(url, path);
            }
            UiEvent::ImageFailed { url, .. } => {
                self.image_errors.insert(url);
            }
            UiEvent::QrSuccess => {
                self.qr_status = Some("登录成功".to_string());
                self.qr_matrix = None;
                self.login_open = false;
                self.login_invalid = false;
            }
            UiEvent::QrError(message) => {
                self.qr_status = Some(message);
                self.qr_matrix = None;
            }
            UiEvent::QrRisk {
                message,
                redirect,
                cookies,
            } => {
                self.qr_status = Some(message.clone());
                self.qr_risk = Some((message, redirect));
                self.qr_risk_cookies = cookies;
                self.qr_matrix = None;
                self.login_invalid = true;
            }
            UiEvent::PhoneCodeSent => {
                self.phone_status = Some("短信验证码已发送".to_string());
                self.phone_captcha_required = false;
                self.qr_risk = None;
                self.qr_status = None;
                self.login_invalid = false;
            }
            UiEvent::PhoneCaptcha { image } => {
                self.phone_captcha = image;
                self.phone_captcha_required = true;
                self.phone_status =
                    Some("需要图形验证码，输入后点击\"验证\"再重新发送".to_string());
            }
            UiEvent::PhoneCaptchaVerified(verified) => {
                self.phone_status = Some(if verified {
                    self.phone_captcha = None;
                    self.phone_captcha_required = false;
                    "图形验证码已通过，请重新发送短信验证码".to_string()
                } else {
                    "图形验证码不正确".to_string()
                });
                self.login_invalid = !verified;
            }
            UiEvent::PhoneLoginSuccess => {
                self.phone_status = Some("手机号登录成功".to_string());
                self.login_status = Some("手机号登录成功".to_string());
                self.login_invalid = false;
                self.phone_captcha = None;
                self.phone_captcha_required = false;
                self.qr_risk = None;
                self.qr_status = None;
                self.login_open = false;
                if self.feed.is_empty() && !self.loading_feed {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                }
            }
            UiEvent::PhoneLoginError(message) => {
                self.phone_status = Some(message);
                self.login_invalid = true;
            }
            UiEvent::Account(profile) => {
                self.login_invalid = false;
                self.login_status = Some(format!("已登录：{}", profile.name));
                self.account = Some(profile);
                if matches!(self.stack.last(), Some(Route::Collections))
                    && self.collections_loading
                    && self.collections.is_empty()
                {
                    if let Some(token) = self.account.as_ref().and_then(|account| {
                        account
                            .url_token
                            .clone()
                            .filter(|token| !token.is_empty())
                            .or_else(|| (!account.id.is_empty()).then(|| account.id.clone()))
                    }) {
                        self.services.load_collections(token, 0, false);
                    }
                }
                if self.feed.is_empty() && !self.loading_feed {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                }
            }
            UiEvent::AccountError(message) => {
                self.login_invalid = true;
                self.login_status = Some(message.clone());
                self.error = Some(message);
                self.loading_feed = false;
            }
            UiEvent::Error(message) => {
                if message.starts_with("热榜") {
                    self.hot_error = Some(message);
                } else if message.starts_with("日报") {
                    self.daily_error = Some(message);
                } else if message.starts_with("推荐流") {
                    self.feed_error = Some(message);
                } else {
                    self.error = Some(message);
                }
                self.loading_hot = false;
                self.loading_daily = false;
                self.loading_feed = false;
            }
            UiEvent::RecommendCollected(report) => {
                self.recommend_collecting = false;
                self.recommend_cancel = None;
                if let Some(error) = &report.error {
                    self.error = Some(format!("本地推荐采集失败：{error}"));
                }
                self.recommend_last_report = Some(report);
                self.invalidate_vectors();
                self.apply_recommendation();
            }
            UiEvent::RecommendEmbeddings(ready) => {
                self.vectors_loading = false;
                if let Some(error) = ready.error {
                    self.error = Some(error);
                }
                if self.vectors_dirty {
                    // The pool changed while this pass ran; discard it and let
                    // the next apply start a fresh pass.
                    self.vectors_dirty = false;
                    self.vector_profile = None;
                } else {
                    self.vector_profile = ready.profile.filter(|profile| profile.item_count() > 0);
                }
                self.apply_recommendation();
            }
            UiEvent::SplashReady {
                image,
                quote,
                source,
            } => {
                // Only takes over the splash if it has not started fading and
                // nothing cached was on screen yet.
                if self.splash_visible && !self.splash_hiding && self.splash_image.is_none() {
                    self.splash_image = Some(image);
                    self.splash_quote = quote;
                    self.splash_source = source;
                }
            }
        }
    }

    fn reload_sample_feed(&mut self) {
        self.blocked_users = self.services.blocked_users();
        self.blocked_topics = self.services.blocked_topics();
        self.items = filter_sample_items(
            m0_sample_feed(),
            &self.blocklist,
            &self.blocked_users,
            &self.blocked_topics,
        );
    }

    fn refresh_after_blocklist(&mut self) {
        self.reload_sample_feed();
        self.loading_hot = true;
        self.loading_daily = true;
        self.services.refresh_hot_search();
        self.services.refresh_daily();
        if self.is_logged_in() {
            self.loading_feed = true;
            self.services.refresh_feed();
        }
    }

    fn open_collections(&mut self, cx: &mut Context<Self>) {
        self.stack.push(Route::Collections);
        self.collections.clear();
        self.collections_offset = 0;
        self.collections_end = false;
        self.collections_error = None;
        let token = self.account.as_ref().and_then(|account| {
            account
                .url_token
                .clone()
                .or_else(|| (!account.id.is_empty()).then(|| account.id.clone()))
        });
        if !self.is_logged_in() {
            self.collections_loading = false;
            self.collections_error = Some("请先登录后查看收藏".to_string());
        } else if let Some(token) = token {
            self.collections_loading = true;
            self.services.load_collections(token, 0, false);
        } else {
            self.collections_loading = true;
            self.services.validate_login();
        }
        cx.notify();
    }

    fn open_collection(&mut self, id: String, title: String) {
        self.stack.push(Route::Collection {
            id: id.clone(),
            title,
        });
        self.collection_items.clear();
        self.collection_items_offset = 0;
        self.collection_items_end = false;
        self.collection_items_error = None;
        self.collection_items_loading = true;
        self.services.load_collection_detail(id.clone());
        self.services.load_collection_items(id, 0, false);
    }

    fn open_favorite_picker(&mut self, cx: &mut Context<Self>) {
        let Some((kind, id)) = self
            .detail
            .as_ref()
            .map(|detail| (detail.kind, detail.id.clone()))
        else {
            return;
        };
        if !matches!(kind, ContentKind::Answer | ContentKind::Article) {
            self.show_toast("该内容暂不支持收藏", cx);
            return;
        }
        if !self.is_logged_in() {
            self.show_toast("请先登录后收藏", cx);
            return;
        }
        self.stack.push(Route::FavoritePicker);
        self.content_collections.clear();
        self.content_collections_loading = true;
        self.content_collections_error = None;
        self.favorite_busy = false;
        self.services.load_content_collections(kind, id);
        cx.notify();
    }

    fn open_history(&mut self, cx: &mut Context<Self>) {
        self.history = self.services.history(200);
        self.stack.push(Route::History);
        cx.notify();
    }

    fn open_crashes(&mut self, cx: &mut Context<Self>) {
        self.crash_logs = crash::list_crashes();
        self.crash_view = None;
        self.crash_text = None;
        self.crash_status = None;
        self.stack.push(Route::Crashes);
        cx.notify();
    }

    fn open_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.refresh_diagnostics();
        self.stack.push(Route::Diagnostics);
        cx.notify();
    }

    fn refresh_diagnostics(&mut self) {
        let dir = data_dir();
        let (_, total_bytes) = dir_stats(&dir);
        let (image_files, image_bytes) = dir_stats(&dir.join("images"));
        self.diag = Some(DiagSnapshot {
            uptime: self.started_at.elapsed(),
            data_dir: dir,
            total_bytes,
            image_files,
            image_bytes,
            crash_logs: self.crash_logs.len(),
            recommend_mode: self.recommend_mode,
            recommend_preset: self.recommend_preset,
            recommend_feedback: self.recommend_feedback_count,
            recommend_pool_items: self.services.recommend_pool_stats().items,
            recommend_vectors: self.services.recommend_embedding_count(),
            recommend_model: self.services.embedding_model_id().to_string(),
            recommend_embedding_fallback: self.services.embedding_fallback(),
            feed_source: self.feed_source,
            feed_ms: self.feed_loaded_ms,
            detail_ms: self.detail_loaded_ms,
        });
    }

    fn open_notifications(&mut self) {
        self.stack.push(Route::Notifications);
        self.notifications.clear();
        self.notifications_error = None;
        if !self.is_logged_in() {
            self.notifications_loading = false;
            self.notifications_error = Some("请先登录后查看通知".to_string());
        } else {
            self.notifications_loading = true;
            self.notifications_offset = 0;
            self.notifications_end = false;
            self.services.load_notifications(0, false);
        }
    }

    fn start_comment_reply(
        &mut self,
        id: String,
        name: String,
        root_id: String,
        cx: &mut Context<Self>,
    ) {
        self.comment_reply_target = Some(CommentReplyTarget { id, name, root_id });
        self.comment_send_error = None;
        cx.notify();
    }

    fn submit_comment(&mut self, cx: &mut Context<Self>) {
        if self.comment_sending {
            return;
        }
        let text = self.comment_input.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.comment_send_error = Some("请输入评论内容".to_string());
            cx.notify();
            return;
        }
        let Some(Route::Comments { kind, id, .. }) = self.stack.last() else {
            return;
        };
        if !self.is_logged_in() {
            self.comment_send_error = Some("请先登录后评论".to_string());
            cx.notify();
            return;
        }
        let kind = *kind;
        let id = id.clone();
        let target = self.comment_reply_target.clone();
        self.comment_sending = true;
        self.comment_send_error = None;
        self.services.submit_comment(
            kind,
            id,
            text,
            target.as_ref().map(|target| target.id.clone()),
            target.as_ref().map(|target| target.root_id.clone()),
        );
        cx.notify();
    }

    fn flush_pending_comment_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.comment_clear_pending {
            return;
        }
        self.comment_clear_pending = false;
        let input = self.comment_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        });
    }

    fn open_comments(&mut self, kind: ContentKind, id: String, title: String) {
        self.stack.push(Route::Comments {
            kind,
            id: id.clone(),
            title,
            anchor: None,
        });
        self.comment_sort = CommentSort::Default;
        self.comments_error = None;
        self.comment_reply_target = None;
        self.comment_sending = false;
        self.comment_send_error = None;
        self.comment_anchor_root = None;
        self.comment_anchor_target = None;
        self.comment_highlight_id = None;
        self.comments_loading_more = false;
        // Coming back to a page that is still in memory should not reload or
        // collapse the replies the reader had opened.
        if let Some(cached) = self.comments_cache.get(&comments_cache_key(kind, &id)) {
            self.comments = cached.comments.clone();
            self.comments_offset = cached.offset;
            self.comments_end = cached.end;
            self.comments_loading = false;
            return;
        }
        self.comments.clear();
        self.comments_offset = 0;
        self.comments_end = false;
        self.comment_children.clear();
        self.comment_children_end.clear();
        self.comment_children_open.clear();
        self.comment_children_loading.clear();
        self.comment_children_error.clear();
        if id == "demo" {
            self.comments = demo_comments();
            self.comments_loading = false;
            self.comments_end = true;
        } else if !self.is_logged_in() {
            self.comments_loading = false;
            self.comments_error = Some("需要登录 Cookie 后才能加载评论".to_string());
        } else {
            self.comments_loading = true;
            self.services.load_comments(kind, id, 0, false);
        }
    }

    /// Store the current comment list so the next visit renders instantly.
    fn remember_comments(&mut self) {
        let Some(Route::Comments { kind, id, .. }) = self.stack.last() else {
            return;
        };
        if self.comments.is_empty() {
            return;
        }
        let key = comments_cache_key(*kind, id);
        if self.comments_cache.len() >= 12 && !self.comments_cache.contains_key(&key) {
            self.comments_cache.clear();
        }
        self.comments_cache.insert(
            key,
            CommentsCacheEntry {
                comments: self.comments.clone(),
                offset: self.comments_offset,
                end: self.comments_end,
            },
        );
    }

    /// Refresh the comments page for the current route, dropping the cache.
    fn reload_comments(&mut self) {
        let Some(Route::Comments { kind, id, .. }) = self.stack.last().cloned() else {
            return;
        };
        self.comments_cache.remove(&comments_cache_key(kind, &id));
        self.comments.clear();
        self.comments_offset = 0;
        self.comments_end = false;
        self.comments_loading = true;
        self.comments_error = None;
        self.services.load_comments(kind, id, 0, false);
    }
    fn search_options(&self) -> SearchOptions {
        SearchOptions {
            vertical: self.search_vertical,
            sort: self.search_sort,
        }
    }

    fn restart_search(&mut self, cx: &mut Context<Self>) {
        let (query, member) = match self.stack.last() {
            Some(Route::Search { query, member }) => (query.clone(), member.clone()),
            _ => return,
        };
        self.search_results.clear();
        self.search_offset = 0;
        self.search_end = false;
        self.search_loading = true;
        self.search_loading_more = false;
        self.search_error = None;
        self.search_results_error = None;
        if let Some((member_hash, _)) = member {
            self.services
                .search_member(query, member_hash, 0, false, self.search_options());
        } else {
            self.services.search(query, 0, false, self.search_options());
        }
        cx.notify();
    }

    fn open_member_search(&mut self, member_hash: String, name: String, query: String) {
        self.services.add_search_history(&query);
        self.search_history = self.services.search_history(8);
        self.stack.push(Route::Search {
            query: query.clone(),
            member: Some((member_hash.clone(), name)),
        });
        self.search_suggestions.clear();
        self.search_results.clear();
        self.search_loading = true;
        self.search_error = None;
        self.search_results_error = None;
        self.search_offset = 0;
        self.search_end = false;
        self.search_loading_more = false;
        self.search_vertical = SearchVertical::All;
        self.search_sort = SearchSort::Default;
        self.services.search_suggest(query.clone());
        self.services
            .search_member(query, member_hash, 0, false, SearchOptions::default());
    }

    fn open_comment_anchor(
        &mut self,
        kind: ContentKind,
        id: String,
        title: String,
        comment_id: String,
    ) {
        self.open_comments(kind, id, title);
        if let Some(Route::Comments { anchor, .. }) = self.stack.last_mut() {
            *anchor = Some(comment_id.clone());
        }
        self.services.load_comment_anchor(comment_id);
    }

    fn open_notification(
        &mut self,
        notification: &zhihu_types::NotificationEntry,
        cx: &mut Context<Self>,
    ) {
        if let Some(anchor) = notification.comment_anchor() {
            if anchor.kind == ContentKind::Question {
                self.open_question(anchor.content_id, notification.title.clone());
            } else {
                self.open_comment_anchor(
                    anchor.kind,
                    anchor.content_id,
                    notification.title.clone(),
                    anchor.comment_id,
                );
            }
            return;
        }
        if let Some((kind, id)) = notification.content_target() {
            if kind == ContentKind::Question {
                self.open_question(id, notification.title.clone());
            } else {
                self.open_content(kind, id, notification.title.clone());
            }
            return;
        }
        if let Some(token) = &notification.actor_token {
            self.open_member(token.clone(), notification.actor.clone());
        } else if let Some(link) = &notification.target_link {
            open_external(link);
        }
        cx.notify();
    }

    fn open_search(&mut self, query: String) {
        self.services.add_search_history(&query);
        self.search_history = self.services.search_history(8);
        self.stack.push(Route::Search {
            query: query.clone(),
            member: None,
        });
        self.search_suggestions.clear();
        self.search_results.clear();
        self.search_loading = true;
        self.search_error = None;
        self.search_results_error = None;
        self.search_offset = 0;
        self.search_end = false;
        self.search_loading_more = false;
        self.search_vertical = SearchVertical::All;
        self.search_sort = SearchSort::Default;
        self.services.search_suggest(query.clone());
        self.services
            .search(query, 0, false, SearchOptions::default());
    }

    /// Open the search page without a query; it shows history and hot terms.
    fn open_search_entry(&mut self, cx: &mut Context<Self>) {
        if matches!(self.stack.last(), Some(Route::Search { .. })) {
            return;
        }
        self.stack.push(Route::Search {
            query: String::new(),
            member: None,
        });
        self.search_suggestions.clear();
        self.search_results.clear();
        self.search_loading = false;
        self.search_error = None;
        self.search_results_error = None;
        self.search_offset = 0;
        self.search_end = true;
        self.search_loading_more = false;
        cx.notify();
    }

    /// Enter from the header input: replace the current search route instead
    /// of stacking a second one.
    fn submit_search_query(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        if let Some(Route::Search {
            query: current,
            member,
        }) = self.stack.last_mut()
        {
            *current = query.clone();
            *member = None;
            self.services.add_search_history(&query);
            self.search_history = self.services.search_history(8);
            self.search_suggestions.clear();
            self.search_results.clear();
            self.search_loading = true;
            self.search_error = None;
            self.search_results_error = None;
            self.search_offset = 0;
            self.search_end = false;
            self.search_loading_more = false;
            self.search_vertical = SearchVertical::All;
            self.search_sort = SearchSort::Default;
            self.services.search_suggest(query.clone());
            self.services
                .search(query, 0, false, SearchOptions::default());
        } else {
            self.open_search(query);
        }
        cx.notify();
    }

    fn tick_hot_rotation(&mut self, cx: &mut Context<Self>) {
        // The carousel only exists on the home feed's search pill; rotating it
        // from any other page or in the background just forces repaints.
        if !self.stack.is_empty() || self.blocklist_open || self.tab != 0 || self.splash_visible {
            return;
        }
        if self.hot.len() < 2 {
            return;
        }
        if self
            .hot_rotate_at
            .is_none_or(|at| at.elapsed().as_millis() >= 3_000)
        {
            self.hot_rotate_index = (self.hot_rotate_index + 1) % self.hot.len();
            self.hot_rotate_at = Some(std::time::Instant::now());
            cx.notify();
        }
    }

    /// Whether anything on screen needs the fast tick (animations, loading,
    /// countdowns). Everything else can run on the 1 s idle cadence.
    fn ui_busy(&self) -> bool {
        self.splash_visible
            || self.pending_read.is_some()
            || self.toast_message.is_some()
            || self.speaking
            || self.tts_active
            || self.action_busy
            || self.loading_feed
            || self.feed_loading_more
            || self.comments_loading
            || self.search_loading
            || self.recommend_collecting
            || self.vectors_loading
    }

    /// Whether the reader has already opened this content.
    fn is_read(&self, kind: ContentKind, id: &str) -> bool {
        self.read_ids.contains(&format!("{}:{}", kind.as_str(), id))
    }

    /// Mark the opened content read once it has stayed on screen for a
    /// second; backing out sooner cancels the mark.
    fn tick_pending_read(&mut self) {
        let Some((kind, id, title, at)) = self.pending_read.clone() else {
            return;
        };
        let current = self.stack.last().and_then(route_content_target);
        if current.as_ref() != Some(&(kind, id.clone())) {
            self.pending_read = None;
            return;
        }
        if at.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.pending_read = None;
        if self.read_ids.insert(format!("{}:{}", kind.as_str(), id)) {
            self.services.mark_read(kind, &id, &title);
        }
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn poll_deeplink(&mut self, cx: &mut Context<Self>) {
        let url = PENDING_DEEPLINK
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(url) = url {
            if self.open_deeplink(&url) {
                cx.notify();
            }
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn poll_deeplink(&mut self, _cx: &mut Context<Self>) {}

    /// Show the cold-start poster for a beat, then fade it into the home page.
    fn tick_splash(&mut self, cx: &mut Context<Self>) {
        if !self.splash_visible || !self.splash_armed {
            return;
        }
        let now = std::time::Instant::now();
        if !self.splash_hiding {
            let elapsed = now.duration_since(self.splash_started_at);
            // Repaint once per second so the skip pill counts down.
            let sec = splash_remaining_secs(elapsed);
            if sec != self.splash_shown_sec {
                self.splash_shown_sec = sec;
                cx.notify();
            }
            if elapsed >= SPLASH_DURATION {
                self.begin_splash_fade(now);
                cx.notify();
            }
        } else if self.splash_hide_at.is_some_and(|at| now >= at) {
            self.splash_visible = false;
            self.splash_hide_at = None;
            // Rotate the pool now that the poster is off screen: drop the one
            // that was just shown and fetch a replacement while the app idles.
            self.services.after_splash_shown();
            cx.notify();
        }
    }

    /// Skip the poster immediately; the fade is the same one used at the end
    /// of the timer so both paths look identical.
    fn skip_splash(&mut self, cx: &mut Context<Self>) {
        if !self.splash_visible || self.splash_hiding || !self.splash_armed {
            return;
        }
        self.begin_splash_fade(std::time::Instant::now());
        cx.notify();
    }

    fn begin_splash_fade(&mut self, now: std::time::Instant) {
        self.splash_hiding = true;
        self.splash_fade_seq = self.splash_fade_seq.wrapping_add(1).max(1);
        self.splash_hide_at = Some(now + SPLASH_FADE);
    }

    fn render_splash(&mut self, cx: &mut Context<Self>) -> Div {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let mut root = div().absolute().top_0().left_0().size_full().bg(bg);
        if let Some(path) = &self.splash_image {
            root = root.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
            );
        }
        let fallback_index = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as usize % SPLASH_QUOTES.len())
            .unwrap_or(0);
        let (quote, source) = if self.splash_quote.is_empty() {
            (
                SPLASH_QUOTES[fallback_index].0.to_string(),
                SPLASH_QUOTES[fallback_index].1.to_string(),
            )
        } else {
            (self.splash_quote.clone(), self.splash_source.clone())
        };
        root.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(220.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x000000a6), 0.),
                    linear_color_stop(rgba(0x00000000), 1.),
                )),
        )
        .child(
            // Transparent-to-black so the quote stays readable without a hard
            // black band cutting the poster in half.
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(px(620.))
                .bg(linear_gradient(
                    0.,
                    linear_color_stop(rgba(0x000000d9), 0.),
                    linear_color_stop(rgba(0x00000000), 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .flex_col()
                .justify_between()
                .px_6()
                .pt(px(safe_area().0 + 12.))
                .pb(px(safe_area().1 + 12.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().size(px(8.)).rounded_full().bg(rgb(0xf0b90b)))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf5f5f5))
                                        .child("知乎 Rust"),
                                ),
                        )
                        .child(
                            div()
                                .id("splash-skip")
                                .px_4()
                                .py_2()
                                .rounded_full()
                                .bg(rgba(0x00000059))
                                .border_1()
                                .border_color(rgba(0xffffff33))
                                .text_sm()
                                .text_color(rgb(0xf5f5f5))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.skip_splash(cx);
                                }))
                                .child(SharedString::from(format!(
                                    "跳过 {}",
                                    self.splash_shown_sec.max(1)
                                ))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xffffff))
                                .child(SharedString::from(quote)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgba(0xffffffcc))
                                .child(SharedString::from(source)),
                        ),
                ),
        )
    }

    /// Consume cookies handed back by the Android WebView (login or risk check).
    fn poll_imported_login(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "android")]
        {
            let Some(imported) = crate::android_login::take_imported() else {
                return;
            };
            match imported {
                crate::android_login::ImportedCookies::WebLogin(raw) => {
                    let count = self.services.import_cookie_header(&raw);
                    if count > 0 {
                        self.services.validate_login();
                        self.loading_feed = true;
                        self.services.refresh_feed();
                        self.login_open = false;
                        self.login_status = Some(format!("已导入 {count} 个 Cookie"));
                        self.show_toast("已导入登录状态", cx);
                    } else {
                        self.login_status = Some("未能从浏览器读取到登录状态".to_string());
                    }
                }
                crate::android_login::ImportedCookies::Session(raw) => {
                    self.qr_risk_cookies = cookie_pairs(&raw);
                    self.qr_risk = None;
                    self.qr_status = Some("验证状态已同步，正在重新获取二维码…".to_string());
                    self.services
                        .start_qr_login_with_cookies(self.qr_risk_cookies.clone());
                    self.show_toast("验证完成，请重新扫码", cx);
                }
            }
            cx.notify();
        }
        #[cfg(not(target_os = "android"))]
        let _ = cx;
    }

    #[cfg(target_os = "android")]
    fn open_web_login(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "android")]
        {
            match crate::android_login::open_web_login() {
                Ok(()) => {
                    self.login_status = Some("请在浏览器窗口中完成登录".to_string());
                }
                Err(error) => {
                    self.login_status = Some(format!("打开浏览器登录失败：{error}"));
                }
            }
        }
        cx.notify();
    }

    fn search_from_history(&mut self, query: String, cx: &mut Context<Self>) {
        if let Some(Route::Search { query: current, .. }) = self.stack.last_mut() {
            *current = query.clone();
        } else {
            return;
        }
        self.services.add_search_history(&query);
        self.search_history = self.services.search_history(8);
        self.services.search_suggest(query);
        self.restart_search(cx);
    }

    fn is_logged_in(&self) -> bool {
        self.account.is_some() || self.services.is_logged_in()
    }

    fn account_label(&self) -> String {
        if self.login_invalid {
            return "登录失效".to_string();
        }
        if let Some(account) = &self.account {
            return account.name.chars().take(6).collect();
        }
        if !self.services.is_logged_in() {
            return "登录".to_string();
        }
        match self.services.current_account_name() {
            Some(name) if !name.is_empty() && name != "default" => name.chars().take(6).collect(),
            _ => "已登录".to_string(),
        }
    }

    /// Avatar of the signed-in account, when the profile loaded.
    fn avatar_url(&self) -> Option<String> {
        self.account
            .as_ref()
            .and_then(|account| account.avatar_url.clone())
            .filter(|url| !url.is_empty())
    }

    pub(crate) fn toggle_theme(&mut self) {
        self.dark = !self.dark;
        self.persist_settings();
    }

    pub(crate) fn set_font_size(&mut self, size: f32) {
        self.font_size = size;
        self.typography_preset = TypographyPreset::Custom;
        self.persist_settings();
    }

    pub(crate) fn set_line_height(&mut self, height: f32) {
        self.line_height = height;
        self.typography_preset = TypographyPreset::Custom;
        self.persist_settings();
    }

    pub(crate) fn toggle_reading_indent(&mut self) {
        self.reading_indent = !self.reading_indent;
        self.persist_settings();
    }

    pub(crate) fn toggle_pos_colors(&mut self) {
        self.pos_colors = !self.pos_colors;
        self.persist_settings();
    }

    pub(crate) fn toggle_hide_mcn(&mut self) {
        self.hide_mcn = !self.hide_mcn;
        self.persist_settings();
        self.apply_recommendation();
    }

    pub(crate) fn typography(&self) -> Typography {
        Typography::from_settings(self.typography_preset, self.font_size, self.line_height)
    }

    pub(crate) fn apply_typography_preset(
        &mut self,
        preset: TypographyPreset,
        cx: &mut Context<Self>,
    ) {
        self.typography_preset = preset;
        if let Some((body, line)) = preset.canonical() {
            self.font_size = body;
            self.line_height = line;
        }
        self.persist_settings();
        cx.notify();
    }

    fn persist_settings(&self) {
        self.services.save_settings(&AppSettings {
            dark: self.dark,
            font_size: self.font_size,
            line_height: self.line_height,
            tab: self.tab,
            tts_rate: self.tts_rate,
            tts_pitch: self.tts_pitch,
            tts_volume: self.tts_volume,
            tts_voice: self.tts_voice.clone(),
            recommend_mode: self.recommend_mode,
            recommend_preset: self.recommend_preset,
            typography: self.typography_preset.as_str().to_string(),
            reading_indent: self.reading_indent,
            pos_colors: self.pos_colors,
            hide_mcn: self.hide_mcn,
        });
    }

    /// Rebuild the displayed feed from the local pool (local mode) or the
    /// server order plus local interest (hybrid / fallback).
    fn apply_recommendation(&mut self) {
        let now = unix_now();
        self.recommend_pool = self.services.recommend_pool_stats();
        self.recommend_vectors = self.services.recommend_embedding_count();
        self.ensure_vectors();
        let interest = InterestProfile::from_feedback(&self.services.recommend_feedback(2000), now);
        let vectors = self.vector_profile.as_ref();

        if self.recommend_mode == RecommendMode::Local {
            let candidates = self.services.recommend_pool(400);
            let mut local = rank_pool_with_vectors(
                &candidates,
                &interest,
                self.recommend_preset,
                now,
                200,
                vectors,
            );
            self.filter_feed_items(&mut local);
            if local.len() >= LOCAL_FEED_MIN_ITEMS {
                self.feed = local;
                self.feed_source = FeedSource::Local;
                return;
            }
        }

        if self.feed_raw.is_empty() {
            self.feed.clear();
            self.feed_source = match self.recommend_mode {
                RecommendMode::Off => FeedSource::Server,
                RecommendMode::Hybrid => FeedSource::Hybrid,
                RecommendMode::Local => FeedSource::LocalFallback,
            };
            return;
        }
        let mut items = self.feed_raw.clone();
        self.filter_feed_items(&mut items);
        if self.recommend_mode != RecommendMode::Off {
            rerank_with_vectors(
                &mut items,
                &interest,
                self.recommend_mode,
                self.recommend_preset,
                vectors,
            );
        }
        self.feed = items;
        self.feed_source = match self.recommend_mode {
            RecommendMode::Off => FeedSource::Server,
            RecommendMode::Hybrid => FeedSource::Hybrid,
            RecommendMode::Local => FeedSource::LocalFallback,
        };
    }

    /// Kick off a background embedding pass when the pool changed and no
    /// profile is ready yet. The UI never blocks on model inference.
    fn ensure_vectors(&mut self) {
        if self.recommend_mode == RecommendMode::Off
            || self.vector_profile.is_some()
            || self.vectors_loading
            || self.recommend_pool.items == 0
        {
            return;
        }
        if self.services.start_recommend_embedding() {
            self.vectors_loading = true;
        }
    }

    fn invalidate_vectors(&mut self) {
        self.vector_profile = None;
        self.vectors_dirty = true;
    }

    fn filter_feed_items(&self, items: &mut Vec<FeedItem>) {
        let hidden_users: HashSet<String> = self
            .hidden_recommend_users
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        if !hidden_users.is_empty() {
            items.retain(|item| {
                !hidden_users.contains(&item.author.id) && !hidden_users.contains(&item.author.name)
            });
        }
        if self.hide_mcn {
            items.retain(|item| !item.author.is_mcn);
        }
        let disliked: HashSet<String> = self.services.disliked_item_ids().into_iter().collect();
        if !disliked.is_empty() {
            items.retain(|item| {
                !disliked.contains(&format!("{}:{}", action_kind(item.kind), item.id))
            });
        }
    }

    pub(crate) fn set_recommend_mode(&mut self, mode: RecommendMode, cx: &mut Context<Self>) {
        self.recommend_mode = mode;
        self.persist_settings();
        self.apply_recommendation();
        cx.notify();
    }

    pub(crate) fn set_recommend_preset(&mut self, preset: RecommendPreset, cx: &mut Context<Self>) {
        self.recommend_preset = preset;
        self.persist_settings();
        self.apply_recommendation();
        cx.notify();
    }

    fn card_offset(&self, id: &str) -> f32 {
        if self.card_swipe_open.as_deref() == Some(id) {
            168.0
        } else if self.card_swipe_id.as_deref() == Some(id) {
            self.card_swipe_dx
        } else {
            0.0
        }
    }

    fn start_card_swipe(&mut self, id: String, x: f32) {
        if self.card_swipe_open.as_deref() != Some(id.as_str()) {
            self.card_swipe_open = None;
        }
        self.card_swipe_id = Some(id);
        self.card_swipe_start_x = x;
        self.card_swipe_dx = 0.0;
    }

    fn start_feed_gesture(&mut self, x: f32, y: f32) {
        self.gesture_pending = true;
        self.gesture_start_x = x;
        self.pull_start_y = y;
        if self.card_swipe_id.is_none() {
            self.card_swipe_start_x = x;
        }
    }

    /// Update the card swipe or pull-to-refresh gesture. Returns whether a
    /// redraw is needed. A mostly vertical drag becomes pull-to-refresh from
    /// anywhere in the feed; a mostly horizontal drag reveals card actions.
    fn update_feed_drag(&mut self, x: f32, y: f32) -> bool {
        if self.gesture_pending {
            let dx = x - self.gesture_start_x;
            let dy = y - self.pull_start_y;
            if dy > 14.0 && dy > dx.abs() {
                self.gesture_pending = false;
                self.card_swipe_id = None;
                self.card_swipe_dx = 0.0;
                self.pull_active = true;
                self.pull_start_y = y - dy.max(0.0);
            } else if dx.abs() > 14.0 && dx.abs() > dy.abs() {
                self.gesture_pending = false;
            } else {
                return false;
            }
        }
        if self.pull_active {
            self.pull_offset = (y - self.pull_start_y).clamp(0.0, 90.0);
            return true;
        }
        if self.card_swipe_id.is_some() {
            let dx = (x - self.card_swipe_start_x).clamp(0.0, 168.0);
            self.card_swipe_dx = dx;
            return true;
        }
        false
    }

    fn finish_feed_drag(&mut self, cx: &mut Context<Self>) {
        self.gesture_pending = false;
        if let Some(id) = self.card_swipe_id.take() {
            if self.card_swipe_dx >= 90.0 {
                self.card_swipe_open = Some(id);
            } else {
                self.card_swipe_open = None;
            }
            if self.card_swipe_dx > 6.0 {
                self.card_swipe_suppress_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(260));
            }
            self.card_swipe_dx = 0.0;
            cx.notify();
        }
        if self.pull_active {
            let trigger = self.pull_offset >= 50.0;
            self.card_swipe_suppress_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
            self.pull_active = false;
            self.pull_offset = 0.0;
            if trigger {
                self.refresh_current(cx);
            } else {
                cx.notify();
            }
        } else {
            cx.notify();
        }
    }

    fn feed_item_meta(
        &self,
        id: &str,
    ) -> Option<(ContentKind, String, String, Vec<String>, String)> {
        self.feed_raw
            .iter()
            .chain(self.feed.iter())
            .chain(self.items.iter())
            .chain(self.search_results.iter())
            .chain(
                self.follow_feeds
                    .iter()
                    .flat_map(|state| state.items.iter()),
            )
            .find(|item| item.id == id)
            .map(|item| {
                (
                    item.kind,
                    item.author.id.clone(),
                    item.author.name.clone(),
                    item.topics.clone(),
                    item.title.clone(),
                )
            })
    }

    fn open_feed_item(
        &mut self,
        kind: ContentKind,
        id: String,
        title: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .card_swipe_suppress_until
            .is_some_and(|until| std::time::Instant::now() < until)
        {
            return;
        }
        if self.card_swipe_open.as_deref() == Some(id.as_str()) {
            self.card_swipe_open = None;
            cx.notify();
            return;
        }
        self.card_swipe_open = None;
        self.open_content(kind, id, title);
        cx.notify();
    }

    fn dislike_feed_item(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some((kind, author_id, _name, topics, title)) = self.feed_item_meta(id) {
            let mut features = topics;
            features.extend(title_tokens(&title));
            self.services.record_recommend_feedback(
                &format!("{}:{}", action_kind(kind), id),
                "dislike",
                &author_id,
                &features,
                -1.5,
            );
        }
        self.feed_raw.retain(|item| item.id != id);
        self.feed.retain(|item| item.id != id);
        for state in &mut self.follow_feeds {
            state.items.retain(|item| item.id != id);
        }
        self.recommend_feedback_count = self.services.recommend_feedback_count();
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        cx.notify();
    }

    fn block_feed_author(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((_, author_id, author_name, _, _)) = self.feed_item_meta(id) else {
            return;
        };
        self.services
            .block_user(author_id.clone(), author_name.clone());
        self.feed_raw
            .retain(|item| item.author.id != author_id && item.author.name != author_name);
        self.feed
            .retain(|item| item.author.id != author_id && item.author.name != author_name);
        for state in &mut self.follow_feeds {
            state
                .items
                .retain(|item| item.author.id != author_id && item.author.name != author_name);
        }
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        cx.notify();
    }

    fn block_feed_topic(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((_, _, _, topics, _)) = self.feed_item_meta(id) else {
            return;
        };
        let Some(topic) = topics.first().cloned() else {
            return;
        };
        self.services.block_topic(topic.clone(), topic.clone());
        self.feed_raw
            .retain(|item| !item.topics.iter().any(|value| value == &topic));
        self.feed
            .retain(|item| !item.topics.iter().any(|value| value == &topic));
        for state in &mut self.follow_feeds {
            state
                .items
                .retain(|item| !item.topics.iter().any(|value| value == &topic));
        }
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        cx.notify();
    }

    fn follow_state(&self) -> &FollowFeedState {
        &self.follow_feeds[follow_index(self.follow_tab)]
    }

    fn ensure_follow_feed(&mut self, kind: FollowFeedKind) {
        let index = follow_index(kind);
        let state = &mut self.follow_feeds[index];
        if state.initialized || state.loading {
            return;
        }
        state.loading = true;
        state.error = None;
        self.services.load_follow_feed(kind, None, false);
    }

    fn switch_follow_tab(&mut self, kind: FollowFeedKind, cx: &mut Context<Self>) {
        if self.follow_tab == kind {
            return;
        }
        self.follow_tab = kind;
        self.feed_scroll = ScrollHandle::new();
        self.list_epoch = self.list_epoch.wrapping_add(1);
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        self.ensure_follow_feed(kind);
        cx.notify();
    }

    fn refresh_follow_feed(&mut self, cx: &mut Context<Self>) {
        let kind = self.follow_tab;
        let index = follow_index(kind);
        let state = &mut self.follow_feeds[index];
        state.loading = true;
        state.loading_more = false;
        state.error = None;
        self.services.load_follow_feed(kind, None, false);
        cx.notify();
    }

    fn load_more_follow_feed(&mut self, cx: &mut Context<Self>) {
        let kind = self.follow_tab;
        let index = follow_index(kind);
        let state = &mut self.follow_feeds[index];
        if state.is_end || state.loading || state.loading_more {
            return;
        }
        let Some(next) = state.next.clone() else {
            return;
        };
        state.loading_more = true;
        self.services.load_follow_feed(kind, Some(next), true);
        cx.notify();
    }

    fn load_more_feed(&mut self, cx: &mut Context<Self>) {
        if self.tab == 1 {
            self.load_more_follow_feed(cx);
            return;
        }
        if !self.services.is_logged_in()
            || self.feed_raw.is_empty()
            || self.feed_end
            || self.loading_feed
            || self.feed_loading_more
        {
            return;
        }
        self.feed_loading_more = true;
        self.services
            .load_more_feed(self.feed_page.saturating_add(1));
        cx.notify();
    }

    fn maybe_load_more_feed(&mut self, cx: &mut Context<Self>) {
        if !self.stack.is_empty() || !matches!(self.tab, 0 | 1) {
            return;
        }
        // The local feed is finite and never paginates against the server.
        if self.tab == 0 && self.feed_source == FeedSource::Local {
            return;
        }
        let offset = f32::from(self.feed_scroll.offset().y).abs();
        let max = f32::from(self.feed_scroll.max_offset().y);
        // With two columns a short page may not overflow at all; keep fetching
        // until the viewport is filled.
        if max > 0.0 && offset < max - 240.0 {
            return;
        }
        self.load_more_feed(cx);
    }

    pub(crate) fn clear_recommendation_data(&mut self, cx: &mut Context<Self>) {
        self.services.clear_recommend_feedback();
        self.recommend_feedback_count = 0;
        self.apply_recommendation();
        cx.notify();
    }

    pub(crate) fn toggle_recommend_collection(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.services.recommend_collection_enabled();
        self.services.set_recommend_collection_enabled(enabled);
        if !enabled {
            self.recommend_collecting = false;
            if let Some(token) = self.recommend_cancel.take() {
                token.cancel();
            }
        }
        cx.notify();
    }

    /// Manual collection pass. Requires the explicit opt-in toggle.
    pub(crate) fn start_recommend_collection(&mut self, cx: &mut Context<Self>) {
        if self.recommend_collecting {
            return;
        }
        if !self.services.recommend_collection_enabled() {
            self.error = Some("请先开启本地推荐采集".to_string());
            cx.notify();
            return;
        }
        self.recommend_last_report = None;
        self.recommend_collecting = true;
        self.recommend_cancel = Some(
            self.services
                .start_recommend_collection(CollectRequest::default()),
        );
        cx.notify();
    }

    pub(crate) fn cancel_recommend_collection(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.recommend_cancel.take() {
            token.cancel();
        }
        cx.notify();
    }

    pub(crate) fn clear_recommend_pool(&mut self, cx: &mut Context<Self>) {
        self.services.clear_recommend_pool();
        self.recommend_pool = self.services.recommend_pool_stats();
        self.recommend_last_report = None;
        self.invalidate_vectors();
        self.apply_recommendation();
        cx.notify();
    }

    /// Remember that the user opened an item from a local list. The recorded
    /// topics/authors feed the optional on-device reranker. Nothing is stored
    /// while local recommendation is off.
    fn record_read_feedback(&mut self, kind: ContentKind, id: &str) {
        if self.recommend_mode == RecommendMode::Off {
            return;
        }
        let found = self
            .feed_raw
            .iter()
            .chain(self.feed.iter())
            .chain(self.items.iter())
            .chain(self.search_results.iter())
            .chain(
                self.follow_feeds
                    .iter()
                    .flat_map(|state| state.items.iter()),
            )
            .find(|item| item.id == id)
            .map(|item| {
                (
                    item.author.id.clone(),
                    item.topics.clone(),
                    item.title.clone(),
                )
            });
        let Some((author_id, topics, title)) = found else {
            return;
        };
        let mut features = topics;
        features.extend(title_tokens(&title));
        if author_id.is_empty() && features.is_empty() {
            return;
        }
        let item_id = format!("{}:{}", action_kind(kind), id);
        self.services
            .record_recommend_feedback(&item_id, "read", &author_id, &features, 1.0);
        self.recommend_feedback_count = self.services.recommend_feedback_count();
    }

    fn record_detail_action(&mut self, action: &str, weight: f32) {
        if self.recommend_mode == RecommendMode::Off {
            return;
        }
        let Some((item_id, author_id)) = self.detail.as_ref().and_then(|detail| {
            if detail.id == "demo" {
                return None;
            }
            Some((
                format!("{}:{}", action_kind(detail.kind), detail.id),
                detail.author.id.clone(),
            ))
        }) else {
            return;
        };
        self.services
            .record_recommend_feedback(&item_id, action, &author_id, &[], weight);
        self.recommend_feedback_count = self.services.recommend_feedback_count();
    }

    /// Explicit "more like this / less like this" from the content menu.
    fn record_explicit_preference(&mut self, action: &str, weight: f32) {
        if self.recommend_mode == RecommendMode::Off {
            return;
        }
        let Some((item_id, author_id, title)) = self.detail.as_ref().and_then(|detail| {
            if detail.id == "demo" {
                return None;
            }
            Some((
                format!("{}:{}", action_kind(detail.kind), detail.id),
                detail.author.id.clone(),
                detail.title.clone(),
            ))
        }) else {
            return;
        };
        let mut features = title_tokens(&title);
        let topics = self
            .feed_raw
            .iter()
            .chain(self.feed.iter())
            .chain(self.items.iter())
            .find(|item| item.title == title)
            .map(|item| item.topics.clone())
            .unwrap_or_default();
        features.extend(topics);
        self.services
            .record_recommend_feedback(&item_id, action, &author_id, &features, weight);
        self.recommend_feedback_count = self.services.recommend_feedback_count();
        self.apply_recommendation();
    }

    fn select_tab(&mut self, index: usize) {
        self.login_open = false;
        self.blocklist_open = false;
        self.error = None;
        self.tab = index.min(TABS.len() - 1);
        self.list_epoch = self.list_epoch.wrapping_add(1);
        self.feed_scroll = ScrollHandle::new();
        self.pull_active = false;
        self.pull_offset = 0.0;
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        self.persist_settings();
        match index {
            0 => {
                if self.services.is_logged_in() && self.feed.is_empty() && !self.loading_feed {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                }
            }
            1 => self.ensure_follow_feed(self.follow_tab),
            2 if !self.hot_requested || self.hot.is_empty() => {
                self.hot_requested = true;
                self.loading_hot = true;
                self.services.refresh_hot_search();
            }
            3 if !self.daily_requested || self.daily.is_empty() => {
                self.daily_requested = true;
                self.loading_daily = true;
                self.services.refresh_daily();
            }
            _ => {}
        }
    }

    /// Capture-phase horizontal swipe arbitration for the home tabs.
    ///
    /// Runs before the scrollable content, so a horizontal gesture never
    /// reaches the list. The dragged page follows the finger; releasing past
    /// 30% of the width switches tabs, anything shorter springs back.
    fn handle_horizontal_swipe(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(delta) => (delta.x * 20.0, delta.y * 20.0),
        };
        let ended = matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
        if event.touch_phase == TouchPhase::Started {
            self.swipe_accum = 0.0;
            self.swipe_vertical = 0.0;
            self.swipe_decided = false;
            self.swipe_gesture_active = true;
            self.swipe_preview = 0.0;
            self.snap_from = None;
            self.slide_seq = 0;
        }
        // Flings keep arriving as horizontal scroll after the finger lifts.
        // Only a real touch gesture may preview or switch tabs; momentum must
        // not trigger a second navigation.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        if !self.swipe_gesture_active {
            return false;
        }
        if !self.swipe_gesture_active && !ended {
            // A desktop mouse wheel has no Started/Ended markers: every
            // notch is an independent gesture.
            self.swipe_accum = 0.0;
            self.swipe_vertical = 0.0;
            self.swipe_decided = false;
            self.snap_from = None;
            self.slide_seq = 0;
        }
        let width = self.viewport_width.max(1.0);

        if !self.swipe_decided && self.handle_touch_pull(dy, ended, cx) {
            return true;
        }

        if self.swipe_decided {
            // The gesture is locked to the horizontal axis: drag the page and
            // swallow the rest so the list underneath never scrolls.
            self.swipe_accum += dx;
            if ended {
                let progress = (self.swipe_accum / width).clamp(-1.0, 1.0);
                if progress.abs() >= 0.30 && !self.commit_swipe(progress, cx) {
                    self.start_snap_back(progress);
                }
                self.swipe_preview = 0.0;
                self.reset_swipe_gesture();
                cx.notify();
                return true;
            }
            self.swipe_preview = (self.swipe_accum / width).clamp(-1.0, 1.0);
            return true;
        }

        self.swipe_accum += dx;
        self.swipe_vertical += dy;
        let horizontal = self.swipe_accum.abs();
        let vertical = self.swipe_vertical.abs();

        if horizontal >= 16.0 && horizontal > vertical * 1.25 {
            self.swipe_decided = true;
            if ended {
                let progress = (self.swipe_accum / width).clamp(-1.0, 1.0);
                if progress.abs() >= 0.30 && !self.commit_swipe(progress, cx) {
                    self.start_snap_back(progress);
                }
                self.swipe_preview = 0.0;
                self.reset_swipe_gesture();
                cx.notify();
            } else {
                self.swipe_preview = (self.swipe_accum / width).clamp(-1.0, 1.0);
            }
            return true;
        }
        if vertical >= 16.0 && vertical > horizontal {
            // Vertical gesture: the list owns it. Forget horizontal travel so
            // a later sideways flick has to earn the threshold from scratch.
            self.swipe_accum = 0.0;
        }
        if ended {
            self.reset_swipe_gesture();
        }
        false
    }

    /// Switch the active child tab or main tab once the swipe is released
    /// past the threshold. Returns `false` when nothing changed (first/last
    /// tab or a throttle hit), so the caller can spring the page back.
    fn commit_swipe(&mut self, progress: f32, cx: &mut Context<Self>) -> bool {
        let forward = progress < 0.0;
        let allowed = self
            .swipe_last_switch
            .is_none_or(|last| last.elapsed().as_millis() >= 300);
        if !allowed {
            return false;
        }
        // The incoming page starts exactly where the outgoing one ends, so the
        // transition continues from the finger instead of jumping.
        let from = if forward {
            1.0 + progress
        } else {
            progress - 1.0
        };
        self.swipe_last_switch = Some(std::time::Instant::now());
        if self.swipe_child_tab(forward, cx) {
            self.start_slide(from);
            return true;
        }
        let next = if forward {
            (self.tab + 1).min(TABS.len() - 1)
        } else {
            self.tab.saturating_sub(1)
        };
        if next == self.tab {
            return false;
        }
        self.select_tab(next);
        self.start_slide(from);
        true
    }

    fn start_snap_back(&mut self, progress: f32) {
        self.snap_from = Some(progress.clamp(-1.0, 1.0));
        self.snap_seq = self.snap_seq.wrapping_add(1).max(1);
    }

    fn reset_swipe_gesture(&mut self) {
        self.swipe_gesture_active = false;
        self.swipe_decided = false;
        self.swipe_accum = 0.0;
        self.swipe_vertical = 0.0;
    }

    /// Touch-driven pull-to-refresh for the feed tabs. Uses the same
    /// `pull_offset`/`pull_active` state as the desktop drag path so the pull
    /// bar looks identical on both platforms.
    fn handle_touch_pull(&mut self, dy: f32, ended: bool, cx: &mut Context<Self>) -> bool {
        if !self.swipe_gesture_active || !self.stack.is_empty() {
            return false;
        }
        let supports = self.tab == 1 || (self.tab == 0 && !self.feed.is_empty());
        if !supports {
            return false;
        }
        if self.pull_active {
            if ended {
                let trigger = self.pull_offset >= 50.0;
                self.pull_active = false;
                self.pull_offset = 0.0;
                self.reset_swipe_gesture();
                if trigger {
                    self.refresh_current(cx);
                } else {
                    cx.notify();
                }
                return true;
            }
            self.pull_offset = (self.pull_offset + dy * 0.7).clamp(0.0, 90.0);
            if self.pull_offset <= 0.0 {
                self.pull_active = false;
            }
            cx.notify();
            return true;
        }
        let at_top = f32::from(self.feed_scroll.offset().y).abs() <= 1.0;
        if at_top && dy > 1.0 {
            self.pull_active = true;
            self.pull_offset = (dy * 0.7).clamp(0.0, 90.0);
            cx.notify();
            return true;
        }
        false
    }

    /// Child tabs (关注 推荐/动态, 话题, 用户页) consume a horizontal swipe
    /// before the main tab bar does.
    fn swipe_child_tab(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if let Some(route) = self.stack.last().cloned() {
            match route {
                Route::Topic { .. } => {
                    let order = [
                        TopicFeedTab::Discussion,
                        TopicFeedTab::Ideas,
                        TopicFeedTab::Unanswered,
                    ];
                    let index = order
                        .iter()
                        .position(|tab| *tab == self.topic_tab)
                        .unwrap_or(0);
                    let next = if forward {
                        (index + 1).min(order.len() - 1)
                    } else {
                        index.saturating_sub(1)
                    };
                    if next != index {
                        self.switch_topic_tab(order[next], cx);
                        return true;
                    }
                    false
                }
                Route::Member { .. } => {
                    if let Some(next) = self.member_tab.step(forward) {
                        self.switch_member_tab(next, cx);
                        return true;
                    }
                    false
                }
                _ => false,
            }
        } else if self.tab == 1 {
            let next = match self.follow_tab {
                FollowFeedKind::Recommend if forward => Some(FollowFeedKind::Activity),
                FollowFeedKind::Activity if !forward => Some(FollowFeedKind::Recommend),
                _ => None,
            };
            if let Some(kind) = next {
                self.switch_follow_tab(kind, cx);
                return true;
            }
            false
        } else {
            false
        }
    }

    fn start_slide(&mut self, from: f32) {
        self.slide_seq = self.slide_seq.wrapping_add(1).max(1);
        self.slide_from = from;
    }

    /// Observe window touch-selection changes once per window.
    fn watch_touch_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.touch_selection_watch.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let subscription = TextSelection::observe_touch_selection(window, cx, move |cx| {
            if let Some(this) = weak.upgrade() {
                this.update(cx, |this, cx| {
                    this.touch_selection_dirty = true;
                    cx.notify();
                });
            }
        });
        self.touch_selection_watch = Some(subscription);
    }

    /// Mobile long-press selection copies immediately (as the desktop
    /// drag-release does) and opens the note toolbar.
    fn sync_touch_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.touch_selection_dirty {
            return;
        }
        self.touch_selection_dirty = false;
        let text = TextSelection::selected_text(window, cx).trim().to_string();
        if text.is_empty() || self.selection_quote.as_deref() == Some(text.as_str()) {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.selection_quote = Some(text);
        self.notes_error = None;
        self.show_toast("已复制选中内容", cx);
    }

    /// Slide the freshly switched tab in over 220 ms, follow the finger while
    /// dragging, and spring back below the release threshold.
    fn slide_wrap(&self, content: AnyElement, window: &Window, bg: Rgba) -> AnyElement {
        let width = f32::from(window.viewport_size().width).max(1.0);
        let animated = |from: f32, id: SharedString, ms: u64, content: AnyElement| -> AnyElement {
            let distance = from * width;
            let animation = Animation::new(std::time::Duration::from_millis(ms))
                .with_easing(gpui_kit::base::animation::ease_out_cubic);
            div()
                .size_full()
                .overflow_hidden()
                .bg(bg)
                .child(
                    div()
                        .size_full()
                        .with_animation(id, animation, move |element, delta| {
                            element.ml(px(distance * (1.0 - delta)))
                        })
                        .child(content),
                )
                .into_any_element()
        };
        if self.slide_seq != 0 {
            return animated(
                self.slide_from,
                SharedString::from(format!("tab-slide-{}", self.slide_seq)),
                220,
                content,
            );
        }
        if self.swipe_preview.abs() > 0.0005 {
            return div()
                .size_full()
                .overflow_hidden()
                .bg(bg)
                .child(
                    div()
                        .size_full()
                        .ml(px(self.swipe_preview * width))
                        .child(content),
                )
                .into_any_element();
        }
        if let Some(from) = self.snap_from {
            return animated(
                from,
                SharedString::from(format!("tab-snap-{}", self.snap_seq)),
                170,
                content,
            );
        }
        div().size_full().bg(bg).child(content).into_any_element()
    }

    fn swipe_capture_layer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        gpui_kit::canvas(
            |_, _, _| (),
            move |_, _, window, _cx| {
                let weak_wheel = weak.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let weak = weak_wheel.clone();
                    let consumed = weak
                        .update(cx, |this, cx| {
                            let consumed = this.handle_horizontal_swipe(event, cx);
                            if consumed {
                                cx.notify();
                            }
                            consumed
                        })
                        .unwrap_or(false);
                    if consumed {
                        cx.stop_propagation();
                    }
                });

                let weak_move = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture
                        || event.pressed_button != Some(MouseButton::Left)
                    {
                        return;
                    }
                    weak_move
                        .update(cx, |this, cx| {
                            if this.update_feed_drag(
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                            ) {
                                cx.notify();
                            }
                        })
                        .ok();
                });

                let weak_up = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    weak_up
                        .update(cx, |this, cx| this.finish_feed_drag(cx))
                        .ok();
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    fn open_deeplink(&mut self, url: &str) -> bool {
        if let Some(id) = topic_deeplink_target(url) {
            self.open_topic(id, "话题".to_string());
            return true;
        }
        let Some((kind, id)) = deeplink_target(url) else {
            return false;
        };
        self.open_content(kind, id, "来自分享".to_string());
        true
    }

    fn open_content(&mut self, kind: ContentKind, id: String, title: String) {
        // A horizontal tab swipe can also be classified as a tap at its end
        // position; ignore that synthetic click for a short window.
        if self
            .swipe_last_switch
            .is_some_and(|last| last.elapsed().as_millis() < 500)
        {
            return;
        }
        // A double tap can deliver two clicks before the route renders.
        // Opening the same target twice leaves an empty route behind when
        // the user goes back, so ignore a duplicate top-of-stack target.
        if self.stack.last().and_then(Route::id) == Some(id.as_str()) {
            return;
        }
        if kind == ContentKind::Question {
            self.open_question(id, title);
            return;
        }
        self.detail_highlight = None;
        self.pending_highlight_scroll = 0;
        self.record_read_feedback(kind, &id);
        let route = match kind {
            ContentKind::Answer => Route::Answer {
                id: id.clone(),
                title: title.clone(),
            },
            ContentKind::Article => Route::Article {
                id: id.clone(),
                title: title.clone(),
            },
            ContentKind::Pin => Route::Pin {
                id: id.clone(),
                title: title.clone(),
            },
            _ => Route::Other {
                title: title.clone(),
            },
        };
        self.stack.push(route);
        // Mark read only after the reader actually stays on the page for a
        // beat; the tick clears this if they back out sooner.
        self.pending_read = Some((kind, id.clone(), title.clone(), std::time::Instant::now()));
        self.detail = None;
        self.detail_blocks.clear();
        self.detail_error = None;
        self.detail_loading = false;
        self.image_errors.clear();
        self.detail_vote = "neutral".to_string();
        self.detail_following = false;
        self.action_error = None;
        self.detail_offline = false;
        self.detail_menu_open = false;
        self.detail_scroll = ScrollHandle::new();
        self.detail_restore_progress = None;
        self.detail_progress_saved = 0.0;
        self.detail_progress_saved_at = None;

        if id.starts_with("m0-") {
            self.show_demo(title, kind);
        } else if !self.services.is_logged_in() {
            self.detail_error = Some("需要登录 Cookie 后才能加载正文".to_string());
        } else {
            self.detail_restore_progress = Some(self.services.read_progress(kind, &id));
            self.detail_loading = true;
            self.detail_requested_at = Some(std::time::Instant::now());
            self.services.load_detail(kind, id);
        }
    }

    fn accept_detail(&mut self, detail: ContentDetail, offline: bool) {
        if let Some(requested) = self.detail_requested_at.take() {
            self.detail_loaded_ms = Some(requested.elapsed().as_millis());
        }
        let matches_route = self
            .stack
            .last()
            .and_then(Route::id)
            .is_some_and(|id| id == detail.id);
        if matches_route {
            if self
                .detail_highlight
                .as_ref()
                .is_some_and(|highlight| highlight.target_id != detail.id)
            {
                self.detail_highlight = None;
                self.pending_highlight_scroll = 0;
            }
            self.detail_blocks = zhihu_content::parse_html(&detail.content_html).blocks;
            self.detail = Some(detail);
            self.detail_loading = false;
            self.detail_error = None;
            self.detail_offline = offline;
        }
    }

    fn poll_notification_unread(&mut self) {
        if !self.is_logged_in() {
            return;
        }
        let due = self
            .notification_polled_at
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(60));
        if due {
            self.notification_polled_at = Some(std::time::Instant::now());
            self.services.refresh_notification_unread();
        }
    }

    fn tick_detail_progress(&mut self) {
        let max = f32::from(self.detail_scroll.max_offset().y);
        if max <= 1.0 {
            return;
        }
        if let Some(progress) = self.detail_restore_progress.take() {
            self.detail_scroll
                .set_offset(gpui_kit::point(px(0.), px(max * progress.clamp(0.0, 1.0))));
            self.detail_progress_saved = progress;
            return;
        }
        let throttle_elapsed = self
            .detail_progress_saved_at
            .is_none_or(|saved_at| saved_at.elapsed() >= std::time::Duration::from_secs(3));
        if throttle_elapsed {
            self.save_detail_progress();
        }
    }

    fn save_detail_progress(&mut self) {
        let Some(detail) = &self.detail else {
            return;
        };
        let max = f32::from(self.detail_scroll.max_offset().y);
        if max <= 1.0 {
            return;
        }
        let progress = (f32::from(self.detail_scroll.offset().y) / max).clamp(0.0, 1.0);
        if (progress - self.detail_progress_saved).abs() < 0.01 {
            return;
        }
        let kind = detail.kind;
        let id = detail.id.clone();
        let previous = self.detail_progress_saved;
        self.services.save_read_progress(kind, &id, progress);
        if self.recommend_mode != RecommendMode::Off {
            if previous < 0.9 && progress >= 0.9 {
                self.record_detail_action("read_done", 1.2);
            } else if previous < 0.5 && progress >= 0.5 {
                self.record_detail_action("read_half", 0.5);
            }
        }
        self.detail_progress_saved = progress;
        self.detail_progress_saved_at = Some(std::time::Instant::now());
    }

    fn open_daily(&mut self, id: u64, title: String) {
        if matches!(self.stack.last(), Some(Route::Daily { id: current, .. }) if *current == id) {
            return;
        }
        self.stack.push(Route::Daily { id, title });
        self.detail = None;
        self.detail_blocks.clear();
        self.detail_error = None;
        self.detail_loading = true;
        self.detail_vote = "neutral".to_string();
        self.detail_following = false;
        self.action_error = None;
        self.image_errors.clear();
        self.services.load_daily(id);
    }

    fn open_topic(&mut self, id: String, name: String) {
        if id.is_empty() {
            return;
        }
        if matches!(self.stack.last(), Some(Route::Topic { id: current, .. }) if *current == id) {
            return;
        }
        self.stack.push(Route::Topic {
            id: id.clone(),
            name,
        });
        self.topic = None;
        self.topic_loading = true;
        self.topic_error = None;
        self.topic_following = false;
        self.topic_tab = TopicFeedTab::Discussion;
        self.topic_sort = TopicSort::Essence;
        self.topic_items.clear();
        self.topic_next = None;
        self.topic_end = false;
        self.topic_loading_more = false;
        self.topic_feed_error = None;
        self.topic_scroll = ScrollHandle::new();
        self.topic_generation = self.topic_generation.wrapping_add(1);
        self.services.load_topic(id.clone());
        self.services.load_topic_feed(
            id,
            self.topic_tab,
            self.topic_sort,
            None,
            false,
            self.topic_generation,
        );
    }

    fn restart_topic_feed(&mut self) {
        let Some(Route::Topic { id, .. }) = self.stack.last() else {
            return;
        };
        let id = id.clone();
        self.topic_generation = self.topic_generation.wrapping_add(1);
        self.topic_items.clear();
        self.topic_next = None;
        self.topic_end = false;
        self.topic_loading_more = false;
        self.topic_feed_error = None;
        self.topic_scroll = ScrollHandle::new();
        self.services.load_topic_feed(
            id,
            self.topic_tab,
            self.topic_sort,
            None,
            false,
            self.topic_generation,
        );
    }

    fn switch_topic_tab(&mut self, tab: TopicFeedTab, cx: &mut Context<Self>) {
        if self.topic_tab == tab {
            return;
        }
        self.topic_tab = tab;
        self.topic_sort = if tab == TopicFeedTab::Ideas {
            TopicSort::Hot
        } else {
            TopicSort::Essence
        };
        self.restart_topic_feed();
        cx.notify();
    }

    fn switch_topic_sort(&mut self, sort: TopicSort, cx: &mut Context<Self>) {
        if self.topic_sort == sort {
            return;
        }
        self.topic_sort = sort;
        self.restart_topic_feed();
        cx.notify();
    }

    fn load_more_topic_feed(&mut self, cx: &mut Context<Self>) {
        let Some(Route::Topic { id, .. }) = self.stack.last() else {
            return;
        };
        if self.topic_end || self.topic_loading_more {
            return;
        }
        let Some(next) = self.topic_next.clone() else {
            return;
        };
        self.topic_loading_more = true;
        self.services.load_topic_feed(
            id.clone(),
            self.topic_tab,
            self.topic_sort,
            Some(next),
            true,
            self.topic_generation,
        );
        cx.notify();
    }

    fn toggle_topic_follow(&mut self, cx: &mut Context<Self>) {
        let Some(Route::Topic { id, .. }) = self.stack.last() else {
            return;
        };
        if !self.is_logged_in() {
            self.topic_error = Some("请先登录后关注话题".to_string());
            cx.notify();
            return;
        }
        let id = id.clone();
        let follow = !self.topic_following;
        self.topic_following = follow;
        if let Some(topic) = &mut self.topic {
            topic.is_following = follow;
        }
        self.services.follow_topic(id, follow);
        cx.notify();
    }

    fn open_question(&mut self, id: String, title: String) {
        if self.stack.last().and_then(Route::id) == Some(id.as_str()) {
            return;
        }
        self.stack.push(Route::Question {
            id: id.clone(),
            title,
        });
        self.question = None;
        self.question_blocks.clear();
        self.question_loading = true;
        self.question_error = None;
        self.question_answers.clear();
        self.question_offset = 0;
        self.question_end = false;
        self.question_loading_more = false;
        self.question_answers_error = None;
        self.question_sort = QuestionSort::Default;
        self.question_comments.clear();
        self.question_comments_offset = 0;
        self.question_comments_end = false;
        self.question_comments_loading_more = false;
        self.question_comments_error = None;
        self.services.load_question(id.clone());
        self.services
            .load_question_answers(id.clone(), 0, false, "default".to_string());
        if self.is_logged_in() {
            self.question_comments_loading = true;
            self.services
                .load_comments(ContentKind::Question, id, 0, false);
        } else {
            self.question_comments_loading = false;
        }
    }

    fn restart_question_answers(&mut self, sort: QuestionSort, cx: &mut Context<Self>) {
        self.question_sort = sort;
        self.question_answers.clear();
        self.question_offset = 0;
        self.question_end = false;
        self.question_loading_more = false;
        self.question_answers_error = None;
        let id = match self.stack.last() {
            Some(Route::Question { id, .. }) => id.clone(),
            _ => return,
        };
        self.services
            .load_question_answers(id, 0, false, sort.order().to_string());
        cx.notify();
    }

    fn load_more_question_answers(&mut self, cx: &mut Context<Self>) {
        if self.question_end || self.question_loading_more {
            return;
        }
        let id = match self.stack.last() {
            Some(Route::Question { id, .. }) => id.clone(),
            _ => return,
        };
        self.question_loading_more = true;
        self.services.load_question_answers(
            id,
            self.question_offset,
            true,
            self.question_sort.order().to_string(),
        );
        cx.notify();
    }

    fn open_member(&mut self, token: String, name: String) {
        if token.is_empty() {
            return;
        }
        if matches!(self.stack.last(), Some(Route::Member { token: current, .. }) if *current == token)
        {
            return;
        }
        self.stack.push(Route::Member {
            token: token.clone(),
            name,
        });
        self.member = None;
        self.member_loading = true;
        self.member_error = None;
        self.member_tab = MemberTab::Answers;
        self.member_answers.clear();
        self.member_answers_offset = 0;
        self.member_answers_end = false;
        self.member_answers_loading = false;
        self.member_answers_error = None;
        self.member_collections.clear();
        self.member_collections_offset = 0;
        self.member_collections_end = false;
        self.member_collections_loading = false;
        self.member_collections_error = None;
        self.member_columns.clear();
        self.member_columns_offset = 0;
        self.member_columns_end = false;
        self.member_columns_loading = false;
        self.member_columns_error = None;
        self.member_people.clear();
        self.member_people_offset = 0;
        self.member_people_end = false;
        self.member_people_loading = false;
        self.member_people_error = None;
        self.member_following = false;
        self.member_blocking = false;
        self.member_search_open = false;
        self.services.load_member(token.clone());
        self.services.load_member_answers(token, 0, false);
    }

    fn switch_member_tab(&mut self, tab: MemberTab, cx: &mut Context<Self>) {
        if self.member_tab == tab {
            return;
        }
        self.member_tab = tab;
        self.member_answers.clear();
        self.member_answers_offset = 0;
        self.member_answers_end = false;
        self.member_answers_loading = false;
        self.member_answers_error = None;
        self.member_collections.clear();
        self.member_collections_offset = 0;
        self.member_collections_end = false;
        self.member_collections_loading = false;
        self.member_collections_error = None;
        self.member_columns.clear();
        self.member_columns_offset = 0;
        self.member_columns_end = false;
        self.member_columns_loading = false;
        self.member_columns_error = None;
        self.member_people.clear();
        self.member_people_offset = 0;
        self.member_people_end = false;
        self.member_people_loading = false;
        self.member_people_error = None;
        self.load_current_member_tab();
        cx.notify();
    }

    fn load_current_member_tab(&mut self) {
        let (token, member_id) = match self.stack.last() {
            Some(Route::Member { token, .. }) => (
                token.clone(),
                self.member
                    .as_ref()
                    .map(|member| member.id.clone())
                    .unwrap_or_default(),
            ),
            _ => return,
        };
        match self.member_tab {
            MemberTab::Answers => {
                self.member_answers_loading = true;
                self.services.load_member_answers(token, 0, false);
            }
            MemberTab::Articles => {
                self.member_answers_loading = true;
                self.services.load_member_articles(token, 0, false);
            }
            MemberTab::Activities => {
                self.member_answers_loading = true;
                self.services.load_member_activities(token, 0, false);
            }
            MemberTab::Pins => {
                self.member_answers_loading = true;
                self.services.load_member_pins(token, 0, false);
            }
            MemberTab::Questions => {
                self.member_answers_loading = true;
                self.services.load_member_questions(token, 0, false);
            }
            MemberTab::Collections => {
                self.member_collections_loading = true;
                self.services.load_member_collections(token, 0, false);
            }
            MemberTab::Columns => {
                self.member_columns_loading = true;
                self.services.load_member_columns(token, 0, false);
            }
            MemberTab::Followers | MemberTab::Following => {
                self.member_people_loading = true;
                self.services.load_member_people(
                    token,
                    member_id,
                    self.member_tab.following().unwrap_or(false),
                    0,
                    false,
                );
            }
        }
    }

    fn load_more_member_answers(&mut self, cx: &mut Context<Self>) {
        let token = match self.stack.last() {
            Some(Route::Member { token, .. }) => token.clone(),
            _ => return,
        };
        match self.member_tab {
            MemberTab::Answers
            | MemberTab::Articles
            | MemberTab::Activities
            | MemberTab::Pins
            | MemberTab::Questions => {
                if self.member_answers_end || self.member_answers_loading {
                    return;
                }
                self.member_answers_loading = true;
                let offset = self.member_answers_offset;
                match self.member_tab {
                    MemberTab::Answers => self.services.load_member_answers(token, offset, true),
                    MemberTab::Articles => self.services.load_member_articles(token, offset, true),
                    MemberTab::Activities => {
                        self.services.load_member_activities(token, offset, true)
                    }
                    MemberTab::Pins => self.services.load_member_pins(token, offset, true),
                    MemberTab::Questions => {
                        self.services.load_member_questions(token, offset, true)
                    }
                    _ => {}
                }
            }
            MemberTab::Collections => {
                if self.member_collections_end || self.member_collections_loading {
                    return;
                }
                self.member_collections_loading = true;
                self.services
                    .load_member_collections(token, self.member_collections_offset, true);
            }
            MemberTab::Columns => {
                if self.member_columns_end || self.member_columns_loading {
                    return;
                }
                self.member_columns_loading = true;
                self.services
                    .load_member_columns(token, self.member_columns_offset, true);
            }
            MemberTab::Followers | MemberTab::Following => {
                if self.member_people_end || self.member_people_loading {
                    return;
                }
                self.member_people_loading = true;
                let member_id = self
                    .member
                    .as_ref()
                    .map(|member| member.id.clone())
                    .unwrap_or_default();
                self.services.load_member_people(
                    token,
                    member_id,
                    self.member_tab.following().unwrap_or(false),
                    self.member_people_offset,
                    true,
                );
            }
        }
        cx.notify();
    }

    fn toggle_member_follow(&mut self, cx: &mut Context<Self>) {
        let token = match self.stack.last() {
            Some(Route::Member { token, .. }) => token.clone(),
            _ => return,
        };
        if !self.is_logged_in() {
            self.member_error = Some("请先登录".to_string());
            cx.notify();
            return;
        }
        let follow = !self.member_following;
        self.member_following = follow;
        self.services.set_member_follow(token, follow);
        cx.notify();
    }

    fn toggle_member_block(&mut self, cx: &mut Context<Self>) {
        let token = match self.stack.last() {
            Some(Route::Member { token, .. }) => token.clone(),
            _ => return,
        };
        if !self.is_logged_in() {
            self.member_error = Some("请先登录".to_string());
            cx.notify();
            return;
        }
        let block = !self.member_blocking;
        self.member_blocking = block;
        self.services.set_member_block(token, block);
        cx.notify();
    }

    fn toggle_member_recommend_hidden(&mut self, cx: &mut Context<Self>) {
        let Some(member) = &self.member else {
            return;
        };
        let id = member.id.clone();
        let name = member.name.clone();
        let hidden = !self
            .hidden_recommend_users
            .iter()
            .any(|(hidden_id, hidden_name)| hidden_id == &id || hidden_name == &name);
        self.services
            .set_recommend_hidden_user(id.clone(), name.clone(), hidden);
        self.hidden_recommend_users = self.services.recommend_hidden_users();
        self.apply_recommendation();
        self.toast_message = Some(if hidden {
            "✓ 已从推荐流隐藏该用户".to_string()
        } else {
            "已恢复该用户的推荐".to_string()
        });
        self.toast_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        cx.notify();
    }

    fn save_selection_highlight(&mut self, cx: &mut Context<Self>) {
        let Some(quote) = self.selection_quote.clone() else {
            return;
        };
        let Some(detail) = &self.detail else {
            return;
        };
        let target_type = action_kind(detail.kind);
        let target_id = detail.id.clone();
        let title = detail.title.clone();
        let plain = zhihu_content::to_plain_text(&self.detail_blocks);
        let start = plain.find(&quote).unwrap_or(0);
        let end = start.saturating_add(quote.len());
        let note = self.selection_note_input.read(cx).value().to_string();
        let tags = self.selection_tags_input.read(cx).value().to_string();
        match self.services.add_highlight(
            target_type,
            &target_id,
            &title,
            &quote,
            &note,
            &tags,
            &self.selection_color,
            start as i64,
            end as i64,
        ) {
            Ok(_) => {
                self.highlights = self.services.highlights(500);
                self.selection_quote = None;
                self.notes_error = None;
                self.show_toast("✓ 已保存阅读笔记", cx);
            }
            Err(error) => {
                self.notes_error = Some(format!("保存笔记失败：{error}"));
                cx.notify();
            }
        }
    }

    fn open_highlight_source(&mut self, highlight: zhihu_types::Highlight, cx: &mut Context<Self>) {
        let kind = match highlight.target_type.as_str() {
            "answer" => ContentKind::Answer,
            "article" => ContentKind::Article,
            "pin" => ContentKind::Pin,
            "question" => ContentKind::Question,
            _ => return,
        };
        self.open_content(kind, highlight.target_id.clone(), highlight.title.clone());
        self.detail_highlight = Some(highlight);
        self.pending_highlight_scroll = 2;
        cx.notify();
    }

    fn scroll_to_pending_highlight(&mut self) {
        if self.pending_highlight_scroll == 0 {
            return;
        }
        let Some(highlight) = self.detail_highlight.as_ref() else {
            self.pending_highlight_scroll = 0;
            return;
        };
        if self
            .detail
            .as_ref()
            .is_none_or(|detail| detail.id != highlight.target_id)
        {
            return;
        }
        if self.pending_highlight_scroll > 1 {
            self.pending_highlight_scroll -= 1;
            return;
        }
        let Some(content_bounds) = self.detail_scroll.bounds_for_item(0) else {
            return;
        };
        let viewport = self.detail_scroll.bounds();
        let total = zhihu_content::to_plain_text(&self.detail_blocks)
            .len()
            .max(1) as f32;
        let ratio = (highlight.start_offset.max(0) as f32 / total).clamp(0.0, 1.0);
        let scrollable = (content_bounds.size.height - viewport.size.height).max(px(0.0));
        let y = (-(scrollable * ratio) + viewport.size.height * 0.3).min(px(0.0));
        self.detail_scroll.set_offset(gpui_kit::point(px(0.0), y));
        self.pending_highlight_scroll = 0;
    }

    fn open_notes(&mut self, cx: &mut Context<Self>) {
        self.stack.push(Route::Notes);
        self.highlights = self.services.highlights(500);
        self.notes_error = None;
        cx.notify();
    }

    fn comment_reaction(&self, id: &str) -> i8 {
        self.comment_votes.get(id).copied().unwrap_or(0)
    }

    fn set_comment_reaction(&mut self, id: &str, target: i8, cx: &mut Context<Self>) {
        let next = if self.comment_reaction(id) == target {
            0
        } else {
            target
        };
        if next == 0 {
            self.comment_votes.remove(id);
        } else {
            self.comment_votes.insert(id.to_string(), next);
        }
        cx.notify();
    }

    fn toggle_comment_like(&mut self, id: &str, liked: bool, cx: &mut Context<Self>) {
        if !self.is_logged_in() {
            self.comment_action_error = Some("请先登录".to_string());
            cx.notify();
            return;
        }
        let next = !liked;
        if next {
            self.comment_likes.insert(id.to_string());
        } else {
            self.comment_likes.remove(id);
        }
        self.comment_action_error = None;
        self.services.like_comment(id.to_string(), next);
        cx.notify();
    }

    fn open_tts_settings(&mut self, cx: &mut Context<Self>) {
        self.stack.push(Route::TtsSettings);
        cx.notify();
    }

    fn adjust_tts(&mut self, kind: u8, delta: f32, cx: &mut Context<Self>) {
        match kind {
            0 => self.tts_rate = (self.tts_rate + delta).clamp(0.5, 2.0),
            1 => self.tts_pitch = (self.tts_pitch + delta).clamp(0.5, 2.0),
            _ => self.tts_volume = (self.tts_volume + delta).clamp(0.0, 1.0),
        }
        self.persist_settings();
        if self.tts_playing {
            self.speak_tts_index(cx);
        }
        cx.notify();
    }

    fn apply_tts_voice(&mut self, cx: &mut Context<Self>) {
        self.tts_voice = self.tts_voice_input.read(cx).value().trim().to_string();
        self.persist_settings();
        if self.tts_playing {
            self.speak_tts_index(cx);
        }
        self.show_toast("朗读设置已保存", cx);
    }

    fn reset_tts_options(&mut self, cx: &mut Context<Self>) {
        self.tts_rate = 1.0;
        self.tts_pitch = 1.0;
        self.tts_volume = 1.0;
        self.tts_voice.clear();
        self.persist_settings();
        self.show_toast("已恢复默认朗读设置", cx);
    }

    fn preview_tts(&mut self, cx: &mut Context<Self>) {
        speak(
            "这是一段朗读试听，当前使用应用内的语音设置。",
            &self.tts_options(),
        );
        self.show_toast("正在试听", cx);
    }

    fn show_demo(&mut self, title: String, kind: ContentKind) {
        let content_html = DEMO_HTML.to_string();
        self.detail_blocks = zhihu_content::parse_html(&content_html).blocks;
        self.detail = Some(ContentDetail {
            id: "demo".to_string(),
            title,
            author: Author {
                id: "demo".to_string(),
                name: "示例作者".to_string(),
                headline: Some("M2 富文本渲染示例".to_string()),
                url_token: Some("demo".to_string()),
                is_following: false,
                avatar_url: None,
                is_mcn: false,
            },
            kind,
            content_html,
            excerpt: String::new(),
            vote_up_count: 128,
            comment_count: 16,
            vote_state: "neutral".to_string(),
            created_time: 0,
            updated_time: 0,
            question_id: None,
        });
        self.detail_loading = false;
        self.detail_vote = "neutral".to_string();
        self.detail_following = false;
        self.action_error = None;
        self.detail_offline = false;
    }

    fn open_image(&mut self, path: PathBuf) {
        stop_speaking();
        self.speaking = false;
        let mut urls = Vec::new();
        collect_image_urls(&self.detail_blocks, &mut urls);
        let mut paths: Vec<PathBuf> = urls
            .iter()
            .filter_map(|url| self.image_paths.get(url).cloned())
            .collect();
        if paths.is_empty() {
            paths.push(path.clone());
        }
        self.image_view_index = paths.iter().position(|entry| *entry == path).unwrap_or(0);
        self.image_view_list = paths;
        self.image_view = Some(path);
        self.image_zoom = 1.0;
        self.image_last_tap = None;
        self.image_pinch_at = None;
        self.image_scroll
            .set_offset(gpui_kit::point(px(0.), px(0.)));
    }

    fn close_image(&mut self) {
        self.image_view = None;
        self.image_view_list.clear();
        self.image_view_index = 0;
        self.image_zoom = 1.0;
        self.image_last_tap = None;
        self.image_pinch_at = None;
    }

    fn adjust_image_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.image_zoom = (self.image_zoom + delta).clamp(1.0, 4.0);
        if self.image_zoom <= 1.0 {
            self.image_scroll
                .set_offset(gpui_kit::point(px(0.), px(0.)));
        }
        cx.notify();
    }

    /// Two-finger pinch from the platform. Keeps the point under the fingers
    /// fixed while the content scales.
    ///
    /// Scroll offsets are negative once the content is scrolled down/right,
    /// so the content coordinate under the finger is `position - offset`;
    /// after scaling by `ratio` it must land back on the same viewport point:
    /// `offset' = position - content * ratio`, clamped to the scroll range
    /// `[-(viewport * (zoom - 1)), 0]`.
    fn apply_image_pinch(&mut self, delta: f32, x: f32, y: f32, cx: &mut Context<Self>) {
        self.image_pinch_at = Some(std::time::Instant::now());
        let factor = 1.0 + delta;
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let old = self.image_zoom;
        let next = (old * factor).clamp(1.0, 4.0);
        if (next - old).abs() < 0.001 {
            return;
        }
        let ratio = next / old;
        let vw = self.viewport_width.max(1.0);
        let vh = self.viewport_height.max(1.0);
        let offset = self.image_scroll.offset();
        let content_x = x - f32::from(offset.x);
        let content_y = y - f32::from(offset.y);
        self.image_zoom = next;
        if next <= 1.0 + f32::EPSILON {
            self.image_scroll
                .set_offset(gpui_kit::point(px(0.), px(0.)));
        } else {
            let nx = (x - content_x * ratio).clamp(-vw * (next - 1.0), 0.0);
            let ny = (y - content_y * ratio).clamp(-vh * (next - 1.0), 0.0);
            self.image_scroll
                .set_offset(gpui_kit::point(px(nx), px(ny)));
        }
        cx.notify();
    }

    fn step_image(&mut self, forward: bool) {
        if self.image_view_list.len() < 2 {
            return;
        }
        let len = self.image_view_list.len();
        self.image_view_index = if forward {
            (self.image_view_index + 1) % len
        } else {
            (self.image_view_index + len - 1) % len
        };
        self.image_view = self.image_view_list.get(self.image_view_index).cloned();
    }

    fn close_video(&mut self) {
        self.video_url = None;
        self.video_error = None;
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            self.video_handle = None;
            self.video_player = None;
        }
    }

    fn open_video(&mut self, url: String, cx: &mut Context<Self>) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            // Desktop: hand the video to the system browser/player instead of
            // showing an empty in-app overlay.
            open_external(&url);
            let _ = cx;
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            self.close_video();
            self.video_url = Some(url.clone());
            match gpui_mobile::packages::video_player::VideoPlayer::new() {
                Ok(mut player) => {
                    let result = player
                        .set_url(&url)
                        .and_then(|_| player.show_surface(0.0, 0.0, 320.0, 180.0))
                        .and_then(|_| player.play());
                    if let Err(error) = result {
                        self.video_error = Some(format!("视频加载失败：{error}"));
                    } else {
                        self.video_handle = player.platform_view_handle();
                    }
                    self.video_player = Some(player);
                }
                Err(error) => {
                    self.video_error = Some(format!("当前平台不支持内嵌视频：{error}"));
                }
            }
            cx.notify();
        }
    }

    fn back(&mut self) -> bool {
        if self.image_view.is_some() {
            self.close_image();
            return true;
        }
        if self.video_url.is_some() {
            self.close_video();
            return true;
        }
        if self.tts_active {
            stop_speaking();
            self.tts_active = false;
            self.tts_playing = false;
            self.speaking = false;
            self.tts_segments.clear();
            return true;
        }
        if self.crash_view.is_some() {
            self.crash_view = None;
            self.crash_text = None;
            return true;
        }
        stop_speaking();
        self.speaking = false;
        self.detail_menu_open = false;
        self.save_detail_progress();
        if self.stack.pop().is_some() {
            self.sync_detail_with_route();
            true
        } else {
            false
        }
    }

    /// Keep the loaded article when returning from a child page (comments,
    /// share sheet, ...). Only drop it when the route below is a different
    /// answer/article, or no content route at all.
    fn sync_detail_with_route(&mut self) {
        let target = self.stack.last().and_then(route_content_target);
        if let Some((kind, id)) = target {
            let same = self
                .detail
                .as_ref()
                .is_some_and(|detail| detail.kind == kind && detail.id == id);
            if same {
                self.detail_error = None;
                self.detail_loading = false;
                return;
            }
            self.detail = None;
            self.detail_blocks.clear();
            self.detail_error = None;
            self.detail_vote = "neutral".to_string();
            self.detail_following = false;
            self.detail_loading = true;
            self.detail_requested_at = Some(std::time::Instant::now());
            match kind {
                ContentKind::Answer => self.services.load_answer(id),
                ContentKind::Article => self.services.load_article(id),
                ContentKind::Pin => self.services.load_pin(id),
                _ => self.detail_loading = false,
            }
        } else {
            self.detail = None;
            self.detail_blocks.clear();
            self.detail_error = None;
            self.detail_loading = false;
            self.detail_vote = "neutral".to_string();
            self.detail_following = false;
            self.action_error = None;
        }
    }

    /// Dismiss routed pages and overlays, returning to the home shell.
    fn go_root(&mut self) {
        self.blocklist_open = false;
        self.login_open = false;
        if self.stack.is_empty() {
            return;
        }
        stop_speaking();
        self.speaking = false;
        self.save_detail_progress();
        self.stack.clear();
        self.detail = None;
        self.detail_blocks.clear();
        self.detail_error = None;
        self.detail_loading = false;
        self.detail_vote = "neutral".to_string();
        self.detail_following = false;
        self.action_error = None;
    }

    /// Sidebar / shortcut tab switch: leave any routed page first.
    pub(crate) fn switch_tab(&mut self, index: usize, _cx: &mut Context<Self>) {
        self.go_root();
        self.select_tab(index);
    }

    /// Reload the current tab or routed page (Ctrl+R, sidebar refresh).
    pub(crate) fn refresh_current(&mut self, cx: &mut Context<Self>) {
        if !self.stack.is_empty() {
            self.retry_detail(cx);
            cx.notify();
            return;
        }
        match self.tab {
            0 => {
                self.feed_error = None;
                if self.services.is_logged_in() {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                } else {
                    self.reload_sample_feed();
                    self.loading_feed = false;
                }
            }
            1 => self.refresh_follow_feed(cx),
            2 => {
                self.hot_requested = true;
                self.loading_hot = true;
                self.hot_error = None;
                self.services.refresh_hot_search();
            }
            3 => {
                self.daily_requested = true;
                self.loading_daily = true;
                self.daily_error = None;
                self.services.refresh_daily();
            }
            _ => {
                self.error = None;
                if self.services.is_logged_in() {
                    self.services.validate_login();
                }
            }
        }
        cx.notify();
    }

    /// Global keyboard shortcuts shared by the mobile and desktop shells.
    pub(crate) fn handle_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        if mods.secondary() && !mods.alt {
            match key {
                "k" | "f" => {
                    if !matches!(self.stack.last(), Some(Route::Search { .. })) {
                        self.open_search_entry(cx);
                        self.search
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    let handle = self.search.read(cx).focus_handle(cx);
                    window.focus(&handle, cx);
                    return true;
                }
                "1" | "2" | "3" | "4" | "5" => {
                    let index = key.as_bytes()[0] as usize - b'1' as usize;
                    self.switch_tab(index, cx);
                    return true;
                }
                "r" => {
                    self.refresh_current(cx);
                    return true;
                }
                "d" => {
                    self.toggle_theme();
                    return true;
                }
                "c" if window.focused(cx) == Some(self.focus_handle.clone())
                    && !self.detail_blocks.is_empty() =>
                {
                    self.copy_detail_markdown(cx);
                    return true;
                }
                _ => {}
            }
        }
        if mods.alt && key == "left" {
            return self.back();
        }
        if key == "escape" {
            return self.back();
        }
        false
    }

    fn vote(&mut self, target: &str) {
        let Some((kind, id)) = self
            .detail
            .as_ref()
            .map(|detail| (detail.kind, detail.id.clone()))
        else {
            return;
        };
        if id != "demo" && !self.is_logged_in() {
            self.action_error = Some("请先登录".to_string());
            return;
        }
        let old = self.detail_vote.clone();
        let next = if old == target { "neutral" } else { target };
        self.detail_vote = next.to_string();
        if let Some(detail) = &mut self.detail {
            detail.vote_state = next.to_string();
            match (old.as_str(), next) {
                ("up", "neutral") | ("up", "down") => {
                    detail.vote_up_count = detail.vote_up_count.saturating_sub(1);
                }
                (_, "up") => detail.vote_up_count += 1,
                _ => {}
            }
        }
        self.action_error = None;
        if id == "demo" {
            return;
        }
        if next == "up" {
            self.record_detail_action("like", 2.0);
        }
        self.action_busy = true;
        self.services.vote(kind, id, next.to_string());
    }

    fn vote_up(&mut self) {
        self.vote("up");
    }

    fn vote_down(&mut self) {
        self.vote("down");
    }

    fn toggle_follow(&mut self) {
        let Some((id, token)) = self
            .detail
            .as_ref()
            .map(|detail| (detail.id.clone(), detail.author.url_token.clone()))
        else {
            return;
        };
        if id != "demo" && !self.is_logged_in() {
            self.action_error = Some("请先登录".to_string());
            return;
        }
        let Some(token) = token else {
            self.action_error = Some("缺少作者标识".to_string());
            return;
        };
        let follow = !self.detail_following;
        self.detail_following = follow;
        if let Some(detail) = &mut self.detail {
            detail.author.is_following = follow;
        }
        self.action_error = None;
        if id == "demo" {
            return;
        }
        if follow {
            self.record_detail_action("follow", 2.0);
        }
        self.action_busy = true;
        self.services.follow_author(token, follow);
    }

    fn retry_detail(&mut self, cx: &mut Context<Self>) {
        let Some(route) = self.stack.last().cloned() else {
            return;
        };
        match route {
            Route::Answer { id, .. } => {
                self.detail_loading = true;
                self.detail_error = None;
                self.detail_requested_at = Some(std::time::Instant::now());
                self.services.load_answer(id);
            }
            Route::Article { id, .. } => {
                self.detail_loading = true;
                self.detail_error = None;
                self.detail_requested_at = Some(std::time::Instant::now());
                self.services.load_article(id);
            }
            Route::Pin { id, .. } => {
                self.detail_loading = true;
                self.detail_error = None;
                self.detail_requested_at = Some(std::time::Instant::now());
                self.services.load_pin(id);
            }
            Route::Search { query, .. } => {
                self.search_loading = true;
                self.search_error = None;
                self.services.search_suggest(query.clone());
                self.restart_search(cx);
            }
            Route::Notifications => {
                self.notifications_loading = true;
                self.notifications_error = None;
                self.notifications_offset = 0;
                self.notifications_end = false;
                self.services.load_notifications(0, false);
            }
            Route::Member { token, .. } => {
                self.member = None;
                self.member_loading = true;
                self.member_error = None;
                self.member_answers.clear();
                self.member_answers_offset = 0;
                self.member_answers_end = false;
                self.member_answers_loading = false;
                self.member_answers_error = None;
                self.services.load_member(token.clone());
                self.services.load_member_answers(token, 0, false);
            }
            Route::Daily { id, .. } => {
                self.detail = None;
                self.detail_blocks.clear();
                self.detail_loading = true;
                self.detail_error = None;
                self.services.load_daily(id);
            }
            Route::Question { id, .. } => {
                self.question = None;
                self.question_blocks.clear();
                self.question_loading = true;
                self.question_error = None;
                self.question_answers.clear();
                self.question_offset = 0;
                self.question_end = false;
                self.question_loading_more = false;
                self.question_answers_error = None;
                self.services.load_question(id.clone());
                self.services.load_question_answers(
                    id,
                    0,
                    false,
                    self.question_sort.order().to_string(),
                );
            }
            Route::Topic { id, .. } => {
                self.topic = None;
                self.topic_loading = true;
                self.topic_error = None;
                self.topic_generation = self.topic_generation.wrapping_add(1);
                self.topic_items.clear();
                self.topic_next = None;
                self.topic_end = false;
                self.topic_loading_more = false;
                self.topic_feed_error = None;
                self.services.load_topic(id.clone());
                self.services.load_topic_feed(
                    id,
                    self.topic_tab,
                    self.topic_sort,
                    None,
                    false,
                    self.topic_generation,
                );
            }
            Route::Collections => {
                self.collections.clear();
                self.collections_offset = 0;
                self.collections_end = false;
                self.collections_error = None;
                self.collections_loading = true;
                if let Some(token) = self.account.as_ref().and_then(|account| {
                    account
                        .url_token
                        .clone()
                        .or_else(|| (!account.id.is_empty()).then(|| account.id.clone()))
                }) {
                    self.services.load_collections(token, 0, false);
                } else {
                    self.collections_loading = false;
                    self.collections_error = Some("账号资料尚未加载，请返回重试".to_string());
                }
            }
            Route::Collection { id, .. } => {
                self.collection_items.clear();
                self.collection_items_offset = 0;
                self.collection_items_end = false;
                self.collection_items_loading = true;
                self.collection_items_error = None;
                self.services.load_collection_detail(id.clone());
                self.services.load_collection_items(id, 0, false);
            }
            Route::FavoritePicker => {
                if let Some((kind, id)) = self
                    .detail
                    .as_ref()
                    .map(|detail| (detail.kind, detail.id.clone()))
                {
                    self.content_collections_loading = true;
                    self.content_collections_error = None;
                    self.services.load_content_collections(kind, id);
                }
            }
            Route::Login => {}
            Route::TtsSettings => {}
            Route::History => {
                self.history = self.services.history(200);
            }
            Route::Notes => {
                self.highlights = self.services.highlights(500);
            }
            Route::Crashes => {
                self.crash_logs = crash::list_crashes();
                self.crash_view = None;
                self.crash_text = None;
            }
            Route::Diagnostics => {
                self.refresh_diagnostics();
            }
            Route::Comments { id, title, .. } => {
                if id == "demo" {
                    self.comments = demo_comments();
                    self.comments_loading = false;
                    self.comments_error = None;
                    self.comments_end = true;
                } else {
                    self.reload_comments();
                }
                let _ = title;
            }
            Route::Other { title } => {
                self.show_demo(title, ContentKind::Pin);
            }
        }
    }
}

impl Render for ZhihuApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        self.watch_touch_selection(window, cx);
        self.sync_touch_selection(window, cx);
        self.viewport_width = f32::from(window.viewport_size().width);
        self.viewport_height = f32::from(window.viewport_size().height);
        // Start the splash clock with the first painted frame, not with
        // construction: startup work can take longer than the whole splash.
        if self.splash_visible && !self.splash_armed {
            self.splash_armed = true;
            self.splash_started_at = std::time::Instant::now();
            self.splash_shown_sec = splash_remaining_secs(std::time::Duration::ZERO);
        }
        // A stack change is a push (slide in from the right) or a pop (the
        // revealed page eases back in from the left).
        if self.stack.len() != self.route_depth_seen {
            self.route_anim_from = if self.stack.len() > self.route_depth_seen {
                1.0
            } else {
                -0.28
            };
            self.route_depth_seen = self.stack.len();
            self.route_anim_seq = self.route_anim_seq.wrapping_add(1).max(1);
        }
        #[cfg(target_os = "android")]
        crate::android_nav::set_back_depth(
            self.stack.len()
                + usize::from(self.image_view.is_some())
                + usize::from(self.video_url.is_some())
                + usize::from(self.crash_view.is_some())
                + usize::from(self.tts_active),
        );
        let content: AnyElement = if self.image_view.is_some() {
            self.render_image_view(cx)
        } else if self.video_url.is_some() {
            self.render_video(cx)
        } else if !self.stack.is_empty() {
            let page = if matches!(self.stack.last(), Some(Route::Search { .. })) {
                self.render_search(cx)
            } else if matches!(self.stack.last(), Some(Route::Comments { .. })) {
                self.render_comments(window, cx)
            } else if matches!(self.stack.last(), Some(Route::Question { .. })) {
                self.render_question(cx)
            } else if matches!(self.stack.last(), Some(Route::Topic { .. })) {
                self.render_topic(cx)
            } else if matches!(self.stack.last(), Some(Route::Member { .. })) {
                self.render_member(cx)
            } else if matches!(self.stack.last(), Some(Route::Collections)) {
                self.render_collections(cx)
            } else if matches!(self.stack.last(), Some(Route::Collection { .. })) {
                self.render_collection_items(cx)
            } else if matches!(self.stack.last(), Some(Route::FavoritePicker)) {
                self.render_favorite_picker(cx)
            } else if matches!(self.stack.last(), Some(Route::Login)) {
                self.render_login_page(window, cx)
            } else if matches!(self.stack.last(), Some(Route::TtsSettings)) {
                self.render_tts_settings(cx)
            } else if matches!(self.stack.last(), Some(Route::Notifications)) {
                self.render_notifications(cx)
            } else if matches!(self.stack.last(), Some(Route::History)) {
                self.render_history(cx)
            } else if matches!(self.stack.last(), Some(Route::Notes)) {
                self.render_notes(cx)
            } else if matches!(self.stack.last(), Some(Route::Crashes)) {
                self.render_crashes(cx)
            } else if matches!(self.stack.last(), Some(Route::Diagnostics)) {
                self.render_diagnostics(cx)
            } else {
                self.render_detail(cx)
            };
            self.render_sidebar_shell(window, page, cx)
        } else {
            self.render_home(window, cx)
        };
        let content = if self.route_anim_seq == 0 {
            content
        } else {
            let width = f32::from(window.viewport_size().width).max(1.0);
            let from = self.route_anim_from * width;
            let animation = Animation::new(std::time::Duration::from_millis(240))
                .with_easing(gpui_kit::base::animation::ease_out_cubic);
            let bg = if self.dark {
                rgb(0x0b0e11)
            } else {
                rgb(0xfafafa)
            };
            div()
                .size_full()
                .overflow_hidden()
                .bg(bg)
                .child(
                    div()
                        .size_full()
                        .with_animation(
                            SharedString::from(format!("route-slide-{}", self.route_anim_seq)),
                            animation,
                            move |element, delta| element.ml(px(from * (1.0 - delta))),
                        )
                        .child(content),
                )
                .into_any_element()
        };
        let page = self.with_toast(content);
        if !self.splash_visible {
            return page;
        }
        let splash: AnyElement = if self.splash_fade_seq != 0 {
            let animation =
                Animation::new(SPLASH_FADE).with_easing(gpui_kit::base::animation::ease_out_cubic);
            self.render_splash(cx)
                .with_animation(
                    SharedString::from(format!("splash-fade-{}", self.splash_fade_seq)),
                    animation,
                    |element, delta| element.opacity(1.0 - delta),
                )
                .into_any_element()
        } else {
            self.render_splash(cx).into_any_element()
        };
        div()
            .relative()
            .size_full()
            .child(page)
            .child(splash)
            .into_any_element()
    }
}

impl ZhihuApp {
    fn render_notifications(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();

        let unread = self.notifications.iter().any(|item| !item.is_read);
        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("notifications-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("消息")
                    .children((self.notification_unread > 0).then(|| {
                        div()
                            .px_2()
                            .rounded_full()
                            .bg(rgb(0xf6465d))
                            .text_xs()
                            .text_color(rgb(0xffffff))
                            .child(SharedString::from(if self.notification_unread > 99 {
                                "99+".to_string()
                            } else {
                                self.notification_unread.to_string()
                            }))
                    })),
            )
            .child(if unread {
                div()
                    .id("notifications-read-all")
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        for item in &mut this.notifications {
                            item.is_read = true;
                        }
                        this.notifications_status = Some("正在同步…".to_string());
                        this.services.mark_notifications_read();
                        cx.notify();
                    }))
                    .child("全部已读")
            } else {
                div().id("notifications-read-all-none")
            });

        let content: AnyElement = if self.notifications_loading {
            empty_state("加载中…", muted).into_any_element()
        } else if let Some(error) = &self.notifications_error {
            div()
                .size_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xdc2626))
                        .child(SharedString::from(error.clone())),
                )
                .into_any_element()
        } else if self.notifications.is_empty() {
            empty_state("暂无通知", muted).into_any_element()
        } else {
            let weak = cx.entity().downgrade();
            let cards: Vec<AnyElement> = self
                .notifications
                .iter()
                .map(|notification| {
                    notification_card(notification, &weak, surface, fg, muted, border, accent)
                })
                .collect();
            div()
                .id("notifications-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3()
                .children(cards)
                .child(if self.notifications_end {
                    div().id("notifications-end")
                } else {
                    div()
                        .id("notifications-more")
                        .w_full()
                        .py_4()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("notifications-more-button")
                                .text_sm()
                                .text_color(accent)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.notifications_loading_more {
                                        this.notifications_loading_more = true;
                                        this.services
                                            .load_notifications(this.notifications_offset, true);
                                        cx.notify();
                                    }
                                }))
                                .child(if self.notifications_loading_more {
                                    "加载中…"
                                } else {
                                    "加载更多"
                                }),
                        )
                })
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(if let Some(status) = &self.notifications_status {
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(status.clone()))
            } else {
                div()
            })
            .child(content)
            .into_any_element()
    }

    fn render_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.flush_pending_comment_clear(window, cx);
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = match self.stack.last() {
            Some(Route::Comments { title, .. }) => title.clone(),
            _ => String::new(),
        };
        let (route_kind, route_id) = match self.stack.last() {
            Some(Route::Comments { kind, id, .. }) => (*kind, id.clone()),
            _ => (ContentKind::Answer, String::new()),
        };

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("comments-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(format!("评论 · {title}"))),
            );

        let mut sort_row = div()
            .h(px(42.))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface);
        for sort in [
            CommentSort::Default,
            CommentSort::Newest,
            CommentSort::Oldest,
            CommentSort::MostLiked,
        ] {
            let active = self.comment_sort == sort;
            sort_row = sort_row.child(
                div()
                    .id(SharedString::from(format!("comment-sort-{}", sort.label())))
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if active { accent } else { bg })
                    .text_xs()
                    .text_color(if active { rgb(0x0b0e11) } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.comment_sort != sort {
                            this.comment_sort = sort;
                            cx.notify();
                        }
                    }))
                    .child(sort.label()),
            );
        }

        let content: AnyElement = if self.comments_loading {
            empty_state("加载中…", muted).into_any_element()
        } else if let Some(error) = &self.comments_error {
            div()
                .size_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xdc2626))
                        .child(SharedString::from(error.clone())),
                )
                .into_any_element()
        } else if self.comments.is_empty() {
            empty_state("还没有评论", muted).into_any_element()
        } else {
            let weak = cx.entity().downgrade();
            let sorted = sorted_comments(&self.comments, self.comment_sort);
            let mut states: HashMap<String, (i8, bool)> = HashMap::new();
            for comment in &self.comments {
                states.insert(
                    comment.id.clone(),
                    (
                        self.comment_reaction(&comment.id),
                        self.comment_likes.contains(&comment.id),
                    ),
                );
                for child in &comment.children {
                    states.insert(
                        child.id.clone(),
                        (
                            self.comment_reaction(&child.id),
                            self.comment_likes.contains(&child.id),
                        ),
                    );
                }
            }
            for children in self.comment_children.values() {
                for child in children {
                    states.insert(
                        child.id.clone(),
                        (
                            self.comment_reaction(&child.id),
                            self.comment_likes.contains(&child.id),
                        ),
                    );
                }
            }
            let cards: Vec<Div> = sorted
                .iter()
                .map(|comment| {
                    let loaded = self.comment_children.get(&comment.id);
                    let expanded = self.comment_children_open.contains(&comment.id);
                    let children = loaded
                        .map(Vec::as_slice)
                        .unwrap_or(comment.children.as_slice());
                    let children = sorted_comments(children, self.comment_sort);
                    let has_replies = comment.child_count > 0
                        || !children.is_empty()
                        || loaded.is_some_and(|children| !children.is_empty());
                    comment_card(
                        comment,
                        Some(&weak),
                        expanded,
                        &children,
                        self.comment_children_loading.contains(&comment.id),
                        self.comment_children_end
                            .get(&comment.id)
                            .copied()
                            .unwrap_or(!comment.children.is_empty()),
                        self.comment_children_error
                            .get(&comment.id)
                            .map(String::as_str),
                        has_replies,
                        self.is_logged_in(),
                        &states,
                        self.comment_highlight_id.as_deref(),
                        surface,
                        fg,
                        muted,
                        border,
                        accent,
                    )
                })
                .collect();
            let mut column = div()
                .id("comments-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py_3()
                .flex()
                .flex_col()
                .gap_3();
            if home::wide_layout(window) {
                column = column.px_4();
            }
            if let Some(error) = &self.comment_action_error {
                column = column.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.clone())),
                );
            }
            column = column.children(cards);
            if !self.comments_end {
                let id = route_id.clone();
                let kind = route_kind;
                let loading_more = self.comments_loading_more;
                column = column.child(
                    div()
                        .id("comments-more")
                        .w_full()
                        .py_3()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(div().text_sm().text_color(accent).child(if loading_more {
                            "加载中…"
                        } else {
                            "加载更多"
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.comments_loading_more {
                                return;
                            }
                            this.comments_loading_more = true;
                            this.services.load_comments(
                                kind,
                                id.clone(),
                                this.comments_offset,
                                true,
                            );
                            cx.notify();
                        })),
                );
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(sort_row)
            .child(content)
            .child(self.render_comment_composer(cx))
            .into_any_element()
    }

    fn render_comment_composer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let mut composer = div()
            .id("comment-composer")
            .w_full()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .bg(surface)
            .border_t_1()
            .border_color(border)
            .flex()
            .flex_col()
            .gap_1();
        if let Some(target) = &self.comment_reply_target {
            composer = composer.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(format!("回复 @{}", target.name))),
                    )
                    .child(
                        div()
                            .id("comment-reply-cancel")
                            .text_xs()
                            .text_color(muted)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.comment_reply_target = None;
                                cx.notify();
                            }))
                            .child("取消"),
                    ),
            );
        }
        if let Some(error) = &self.comment_send_error {
            composer = composer.child(
                div()
                    .text_xs()
                    .text_color(rgb(0xf6465d))
                    .child(SharedString::from(error.clone())),
            );
        }
        composer
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.comment_input)
                            .flex_1()
                            .h(px(40.))
                            .bg(bg)
                            .text_color(fg),
                    )
                    .child(
                        div()
                            .id("comment-send")
                            .flex_shrink_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(accent)
                            .text_sm()
                            .text_color(rgb(0x0b0e11))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_comment(cx);
                            }))
                            .child(if self.comment_sending {
                                "发送中…"
                            } else {
                                "发送"
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_tts_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("tts-settings-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("朗读设置"),
            );

        let step_row =
            |label: &'static str, value: String, kind: u8, delta: f32, cx: &mut Context<Self>| {
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_sm().text_color(fg).child(label))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id(SharedString::from(format!("tts-minus-{kind}")))
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.adjust_tts(kind, -delta, cx);
                                    }))
                                    .child("−"),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_center()
                                    .text_sm()
                                    .text_color(fg)
                                    .child(SharedString::from(value)),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("tts-plus-{kind}")))
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.adjust_tts(kind, delta, cx);
                                    }))
                                    .child("+"),
                            ),
                    )
            };

        let voice_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().text_color(fg).child("音色 / 语言"))
                    .child(div().text_xs().text_color(muted).child("留空使用系统默认")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.tts_voice_input)
                            .flex_1()
                            .h(px(36.))
                            .rounded_md()
                            .bg(surface)
                            .text_color(fg),
                    )
                    .child(
                        div()
                            .id("tts-voice-apply")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(accent)
                            .text_sm()
                            .text_color(rgb(0x0b0e11))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_tts_voice(cx);
                            }))
                            .child("应用"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Windows 例：Microsoft Kangkang / Microsoft Yaoyao；Android 例：zh-cn-x-ccc-network"),
            );

        let engine = if cfg!(target_os = "windows") {
            "Windows SAPI5（System.Speech，离线）"
        } else if cfg!(target_os = "android") {
            "Android 系统 TextToSpeech（离线/在线取决于系统音色）"
        } else if cfg!(target_os = "macos") {
            "macOS say（离线）"
        } else {
            "spd-say（离线）"
        };

        let body = div()
            .id("tts-settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(step_row(
                "语速",
                format!("{:.1}x", self.tts_rate),
                0,
                0.1,
                cx,
            ))
            .child(step_row(
                "音调",
                format!("{:.1}", self.tts_pitch),
                1,
                0.1,
                cx,
            ))
            .child(step_row(
                "音量",
                format!("{:.0}%", self.tts_volume * 100.0),
                2,
                0.1,
                cx,
            ))
            .child(voice_row)
            .child(
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_sm().text_color(fg).child("引擎"))
                    .child(div().text_xs().text_color(muted).child(engine)),
            )
            .child(
                div()
                    .p_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("tts-preview")
                            .px_4()
                            .py_2()
                            .rounded_full()
                            .bg(accent)
                            .text_sm()
                            .text_color(rgb(0x0b0e11))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview_tts(cx);
                            }))
                            .child("试听"),
                    )
                    .child(
                        div()
                            .id("tts-reset")
                            .px_4()
                            .py_2()
                            .rounded_full()
                            .bg(surface)
                            .border_1()
                            .border_color(border)
                            .text_sm()
                            .text_color(muted)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reset_tts_options(cx);
                            }))
                            .child("恢复默认"),
                    ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_member(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let danger = rgb(0xf6465d);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = match self.stack.last() {
            Some(Route::Member { name, .. }) => name.clone(),
            _ => String::new(),
        };

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("member-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(title)),
            );

        let content: AnyElement = if self.member_loading && self.member.is_none() {
            empty_state("用户加载中…", muted).into_any_element()
        } else {
            let mut column = div()
                .id("member-scroll")
                .size_full()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3();
            if let Some(member) = &self.member {
                let name = member.name.clone();
                let avatar = member
                    .avatar_url
                    .as_ref()
                    .map(|url| {
                        img(url.clone())
                            .size(px(64.))
                            .rounded_full()
                            .into_any_element()
                    })
                    .unwrap_or_else(|| {
                        div()
                            .size(px(64.))
                            .rounded_full()
                            .bg(accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x0b0e11))
                                    .child(SharedString::from(
                                        name.chars().next().unwrap_or('知').to_string(),
                                    )),
                            )
                            .into_any_element()
                    });
                let mut card =
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .rounded_lg()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(avatar)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_lg()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(fg)
                                                .child(SharedString::from(name)),
                                        )
                                        .children(member.is_mcn.then(|| {
                                            div()
                                                .px_1()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(rgb(0x3a2f1b))
                                                .text_xs()
                                                .text_color(rgb(0xf0b90b))
                                                .child("MCN 机构")
                                        }))
                                        .child(div().text_xs().text_color(muted).child(
                                            SharedString::from(
                                                member.headline.clone().unwrap_or_default(),
                                            ),
                                        )),
                                )
                                .child(
                                    div()
                                        .id("member-follow")
                                        .px_4()
                                        .py_1()
                                        .rounded_full()
                                        .bg(if self.member_following {
                                            surface
                                        } else {
                                            accent
                                        })
                                        .border_1()
                                        .border_color(if self.member_following {
                                            border
                                        } else {
                                            accent
                                        })
                                        .text_sm()
                                        .text_color(if self.member_following {
                                            muted
                                        } else {
                                            rgb(0x0b0e11)
                                        })
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_member_follow(cx);
                                        }))
                                        .child(if self.member_following {
                                            "已关注"
                                        } else {
                                            "+ 关注"
                                        }),
                                ),
                        )
                        .child(div().text_xs().text_color(muted).child(SharedString::from(
                            format!(
                                "{} 回答 · {} 文章 · {} 关注者 · {} 关注",
                                member.answer_count,
                                member.articles_count,
                                member.follower_count,
                                member.following_count
                            ),
                        )));
                let description = member.description.trim();
                if !description.is_empty() {
                    card = card.child(
                        div()
                            .text_sm()
                            .text_color(fg)
                            .child(SharedString::from(description.to_string())),
                    );
                }
                let hidden_in_recommend = self
                    .hidden_recommend_users
                    .iter()
                    .any(|(id, hidden_name)| id == &member.id || hidden_name == &member.name);
                card = card.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .id("member-block")
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(surface)
                                .border_1()
                                .border_color(if self.member_blocking { danger } else { border })
                                .text_xs()
                                .text_color(if self.member_blocking { danger } else { muted })
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_member_block(cx);
                                }))
                                .child(if self.member_blocking {
                                    "解除拉黑"
                                } else {
                                    "拉黑"
                                }),
                        )
                        .child(
                            div()
                                .id("member-hide-recommend")
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(surface)
                                .border_1()
                                .border_color(if hidden_in_recommend { accent } else { border })
                                .text_xs()
                                .text_color(if hidden_in_recommend { accent } else { muted })
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_member_recommend_hidden(cx);
                                }))
                                .child(if hidden_in_recommend {
                                    "已屏蔽推荐"
                                } else {
                                    "屏蔽推荐"
                                }),
                        )
                        .child(
                            div()
                                .id("member-search-content")
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(surface)
                                .border_1()
                                .border_color(border)
                                .text_xs()
                                .text_color(accent)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.member_search_open = !this.member_search_open;
                                    cx.notify();
                                }))
                                .child("搜索 TA 的创作"),
                        ),
                );
                if self.member_search_open {
                    let member_hash = member.id.clone();
                    let member_name = member.name.clone();
                    card = card.child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Input::new(&self.member_search_input)
                                    .flex_1()
                                    .h(px(38.))
                                    .bg(bg)
                                    .text_color(fg),
                            )
                            .child(
                                div()
                                    .id("member-search-submit")
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(accent)
                                    .text_sm()
                                    .text_color(rgb(0x0b0e11))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let query = this
                                            .member_search_input
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .to_string();
                                        if query.is_empty() {
                                            return;
                                        }
                                        this.open_member_search(
                                            member_hash.clone(),
                                            member_name.clone(),
                                            query,
                                        );
                                        cx.notify();
                                    }))
                                    .child("搜索"),
                            ),
                    );
                }
                column = column.child(card);
            }
            if let Some(error) = &self.member_error {
                column = column.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.clone())),
                );
            }
            let mut tabs_row = div()
                .id("member-tabs")
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_wrap()
                .gap_2();
            for tab in [
                MemberTab::Answers,
                MemberTab::Articles,
                MemberTab::Activities,
                MemberTab::Pins,
                MemberTab::Questions,
                MemberTab::Collections,
                MemberTab::Columns,
                MemberTab::Followers,
                MemberTab::Following,
            ] {
                let active = self.member_tab == tab;
                tabs_row = tabs_row.child(
                    div()
                        .id(SharedString::from(format!("member-tab-{}", tab.label())))
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .bg(if active { accent } else { surface })
                        .border_1()
                        .border_color(if active { accent } else { border })
                        .text_sm()
                        .text_color(if active { rgb(0x0b0e11) } else { fg })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_member_tab(tab, cx);
                        }))
                        .child(tab.label()),
                );
            }
            column = column.child(tabs_row);

            match self.member_tab {
                MemberTab::Answers
                | MemberTab::Articles
                | MemberTab::Activities
                | MemberTab::Pins
                | MemberTab::Questions => {
                    if let Some(error) = &self.member_answers_error {
                        column = column.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf6465d))
                                .child(SharedString::from(error.clone())),
                        );
                    }
                    if self.member_answers.is_empty() {
                        if !self.member_loading && !self.member_answers_loading {
                            let empty = match self.member_tab {
                                MemberTab::Articles => "暂无文章",
                                MemberTab::Activities => "暂无动态",
                                MemberTab::Pins => "暂无想法",
                                MemberTab::Questions => "暂无提问",
                                _ => "暂无回答",
                            };
                            column = column.child(empty_state(empty, muted));
                        }
                    } else {
                        for item in &self.member_answers {
                            let id = item.id.clone();
                            let kind = item.kind;
                            let item_title = item.title.clone();
                            let details = if kind == ContentKind::Question {
                                format!(
                                    "{} 关注 · {} 回答",
                                    format_count(item.vote_up_count),
                                    format_count(item.comment_count)
                                )
                            } else {
                                format!(
                                    "{} {} · {} 评论",
                                    item.vote_up_count,
                                    if kind == ContentKind::Pin {
                                        "喜欢"
                                    } else {
                                        "赞同"
                                    },
                                    item.comment_count
                                )
                            };
                            column = column.child(
                                div()
                                    .id(SharedString::from(format!("member-content-{}", item.id)))
                                    .w_full()
                                    .flex_shrink_0()
                                    .p_4()
                                    .rounded_lg()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_content(kind, id.clone(), item_title.clone());
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(fg)
                                            .line_clamp(2)
                                            .child(SharedString::from(item.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(SharedString::from(details)),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(muted)
                                            .line_clamp(4)
                                            .child(SharedString::from(item.excerpt.clone())),
                                    ),
                            );
                        }
                    }
                }
                MemberTab::Collections => {
                    if let Some(error) = &self.member_collections_error {
                        column = column.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf6465d))
                                .child(SharedString::from(error.clone())),
                        );
                    }
                    if self.member_collections.is_empty() {
                        if !self.member_collections_loading {
                            column = column.child(empty_state("暂无公开收藏夹", muted));
                        }
                    } else {
                        for collection in &self.member_collections {
                            let id = collection.id.clone();
                            let title = collection.title.clone();
                            column = column.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "member-collection-{}",
                                        collection.id
                                    )))
                                    .w_full()
                                    .flex_shrink_0()
                                    .p_4()
                                    .rounded_lg()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_collection(id.clone(), title.clone());
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(fg)
                                            .child(SharedString::from(collection.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(format!("{} 篇内容", collection.item_count)),
                                    ),
                            );
                        }
                    }
                }
                MemberTab::Columns => {
                    if let Some(error) = &self.member_columns_error {
                        column = column.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf6465d))
                                .child(SharedString::from(error.clone())),
                        );
                    }
                    if self.member_columns.is_empty() {
                        if !self.member_columns_loading {
                            column = column.child(empty_state("暂无专栏", muted));
                        }
                    } else {
                        for member_column in &self.member_columns {
                            let url = format!("https://www.zhihu.com/column/{}", member_column.id);
                            column = column.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "member-column-{}",
                                        member_column.id
                                    )))
                                    .w_full()
                                    .flex_shrink_0()
                                    .p_4()
                                    .rounded_lg()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .cursor_pointer()
                                    .on_click(move |_, _, _| open_external(&url))
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(fg)
                                            .child(SharedString::from(member_column.title.clone())),
                                    )
                                    .child(div().text_xs().text_color(muted).child(format!(
                                        "{} 篇文章 · {} 关注",
                                        member_column.article_count, member_column.follower_count
                                    )))
                                    .children((!member_column.description.is_empty()).then(|| {
                                        div().text_sm().text_color(muted).line_clamp(3).child(
                                            SharedString::from(member_column.description.clone()),
                                        )
                                    })),
                            );
                        }
                    }
                }
                MemberTab::Followers | MemberTab::Following => {
                    if let Some(error) = &self.member_people_error {
                        column = column.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf6465d))
                                .child(SharedString::from(error.clone())),
                        );
                    }
                    if self.member_people.is_empty() {
                        if !self.member_people_loading {
                            column = column.child(empty_state("暂无用户", muted));
                        }
                    } else {
                        for person in &self.member_people {
                            let token = if person.url_token.is_empty() {
                                person.id.clone()
                            } else {
                                person.url_token.clone()
                            };
                            let name = person.name.clone();
                            column = column.child(
                                div()
                                    .id(SharedString::from(format!("member-person-{}", person.id)))
                                    .w_full()
                                    .flex_shrink_0()
                                    .p_3()
                                    .rounded_lg()
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_member(token.clone(), name.clone());
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .size(px(36.))
                                            .rounded_full()
                                            .bg(accent)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(rgb(0x0b0e11))
                                                    .child(SharedString::from(
                                                        person
                                                            .name
                                                            .chars()
                                                            .next()
                                                            .unwrap_or('知')
                                                            .to_string(),
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(fg)
                                                    .child(SharedString::from(person.name.clone())),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted)
                                                    .line_clamp(1)
                                                    .child(SharedString::from(
                                                        person.headline.clone().unwrap_or_default(),
                                                    )),
                                            ),
                                    ),
                            );
                        }
                    }
                }
            }

            let (has_items, is_end, is_loading) = match self.member_tab {
                MemberTab::Answers
                | MemberTab::Articles
                | MemberTab::Activities
                | MemberTab::Pins
                | MemberTab::Questions => (
                    !self.member_answers.is_empty(),
                    self.member_answers_end,
                    self.member_answers_loading,
                ),
                MemberTab::Collections => (
                    !self.member_collections.is_empty(),
                    self.member_collections_end,
                    self.member_collections_loading,
                ),
                MemberTab::Columns => (
                    !self.member_columns.is_empty(),
                    self.member_columns_end,
                    self.member_columns_loading,
                ),
                MemberTab::Followers | MemberTab::Following => (
                    !self.member_people.is_empty(),
                    self.member_people_end,
                    self.member_people_loading,
                ),
            };
            if has_items {
                if is_end {
                    column = column.child(
                        div()
                            .h(px(48.))
                            .flex_shrink_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(muted)
                            .child("没有更多了"),
                    );
                } else {
                    column = column.child(
                        div()
                            .id("member-more")
                            .h(px(48.))
                            .flex_shrink_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(accent)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_more_member_answers(cx);
                            }))
                            .child(if is_loading {
                                "加载中…"
                            } else {
                                "加载更多"
                            }),
                    );
                }
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(content)
            .into_any_element()
    }

    fn render_question(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = match self.stack.last() {
            Some(Route::Question { title, .. }) => title.clone(),
            _ => String::new(),
        };

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("question-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(title.clone())),
            )
            .children(
                self.stack
                    .last()
                    .and_then(|route| match route {
                        Route::Question { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .map(|id| {
                        div()
                            .id("question-log")
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, _| {
                                open_external(&format!("https://www.zhihu.com/question/{id}/log"));
                            }))
                            .child("变更日志")
                    }),
            );

        let mut sort_row = div()
            .h(px(42.))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface);
        for sort in [
            QuestionSort::Default,
            QuestionSort::Newest,
            QuestionSort::MostLiked,
        ] {
            let active = self.question_sort == sort;
            sort_row = sort_row.child(
                div()
                    .id(SharedString::from(format!(
                        "question-sort-{}",
                        sort.label()
                    )))
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if active { accent } else { bg })
                    .text_xs()
                    .text_color(if active { rgb(0x0b0e11) } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.question_sort != sort {
                            this.restart_question_answers(sort, cx);
                        }
                    }))
                    .child(sort.label()),
            );
        }

        let content: AnyElement = if self.question_loading && self.question.is_none() {
            empty_state("问题加载中…", muted).into_any_element()
        } else {
            let mut column = div()
                .id("question-scroll")
                .size_full()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3();
            if let Some(question) = &self.question {
                let description = zhihu_content::to_plain_text(&self.question_blocks);
                let mut card =
                    div()
                        .w_full()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .rounded_lg()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(fg)
                                .child(SharedString::from(question.title.clone())),
                        )
                        .child(div().text_xs().text_color(muted).child(SharedString::from(
                            format!(
                                "{} 个回答 · {} 人关注",
                                question.answer_count, question.follower_count
                            ),
                        )));
                if !description.is_empty() {
                    card = card.child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(SharedString::from(description)),
                    );
                }
                column = column.child(card);
            }
            if let Some(error) = &self.question_error {
                column = column.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.clone())),
                );
            }
            if let Some(error) = &self.question_answers_error {
                column = column.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.clone())),
                );
            }
            let question_id = self
                .stack
                .last()
                .and_then(|route| match route {
                    Route::Question { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let question_title = self
                .question
                .as_ref()
                .map(|question| question.title.clone())
                .unwrap_or_else(|| title.clone());
            let comment_count = self.question_comments.len();
            column = column.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(fg)
                            .child(SharedString::from(format!(
                                "问题评论 · {}",
                                format_count(comment_count as u64)
                            ))),
                    )
                    .child({
                        let qid = question_id.clone();
                        let qtitle = question_title.clone();
                        div()
                            .id("question-all-comments")
                            .text_xs()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_comments(
                                    ContentKind::Question,
                                    qid.clone(),
                                    qtitle.clone(),
                                );
                                cx.notify();
                            }))
                            .child("查看全部")
                    }),
            );
            if self.question_comments_loading && self.question_comments.is_empty() {
                column = column.child(div().text_xs().text_color(muted).child("评论加载中…"));
            } else if let Some(error) = &self.question_comments_error {
                column = column.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.clone())),
                );
            } else if self.question_comments.is_empty() {
                column = column.child(div().text_xs().text_color(muted).child("暂无问题评论"));
            } else {
                for comment in self.question_comments.iter().take(3) {
                    let author_name = comment.author.name.clone();
                    let plain = zhihu_content::to_plain_text(
                        &zhihu_content::parse_html(&comment.content).blocks,
                    );
                    let mut avatar = div()
                        .size(px(22.))
                        .rounded_full()
                        .bg(accent)
                        .flex()
                        .items_center()
                        .justify_center();
                    if let Some(url) = &comment.author.avatar_url {
                        avatar = avatar
                            .bg(surface)
                            .overflow_hidden()
                            .child(img(url.clone()).size(px(22.)).rounded_full());
                    } else {
                        avatar =
                            avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
                    }
                    let qid = question_id.clone();
                    let qtitle = question_title.clone();
                    let reply_id = comment.id.clone();
                    let reply_name = comment.author.name.clone();
                    let reply_root = comment.id.clone();
                    column = column.child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .p_3()
                            .rounded_lg()
                            .bg(surface)
                            .border_1()
                            .border_color(border)
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(avatar)
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(fg)
                                            .child(SharedString::from(author_name)),
                                    )
                                    .child(div().flex_1())
                                    .child(div().text_xs().text_color(muted).child(
                                        SharedString::from(format!("{} 喜欢", comment.like_count)),
                                    )),
                            )
                            .child(emoji_plain_text(&plain, fg, 14.0, 20.0))
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "question-comment-reply-{}",
                                        comment.id
                                    )))
                                    .text_xs()
                                    .text_color(accent)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_comments(
                                            ContentKind::Question,
                                            qid.clone(),
                                            qtitle.clone(),
                                        );
                                        this.comment_reply_target = Some(CommentReplyTarget {
                                            id: reply_id.clone(),
                                            name: reply_name.clone(),
                                            root_id: reply_root.clone(),
                                        });
                                        cx.notify();
                                    }))
                                    .child("回复"),
                            ),
                    );
                }
            }
            if self.question_answers.is_empty() {
                if !self.question_loading {
                    column = column.child(empty_state("暂无回答", muted));
                }
            } else {
                let answer_title = self
                    .question
                    .as_ref()
                    .map(|question| question.title.clone())
                    .unwrap_or_else(|| title.clone());
                for answer in &self.question_answers {
                    let id = answer.id.clone();
                    let answer_title = answer_title.clone();
                    let author_token = answer
                        .author
                        .url_token
                        .clone()
                        .filter(|token| !token.is_empty())
                        .unwrap_or_else(|| answer.author.id.clone());
                    let author_name = answer.author.name.clone();
                    let mut avatar = div()
                        .id(SharedString::from(format!("q-answer-avatar-{}", answer.id)))
                        .size(px(24.))
                        .rounded_full()
                        .bg(accent)
                        .flex()
                        .items_center()
                        .justify_center();
                    if let Some(url) = &answer.author.avatar_url {
                        avatar = avatar
                            .bg(surface)
                            .overflow_hidden()
                            .child(img(url.clone()).size(px(24.)).rounded_full());
                    } else {
                        avatar =
                            avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
                    }
                    if !author_token.is_empty() {
                        let weak = cx.entity().downgrade();
                        avatar = avatar.cursor_pointer().on_click(move |_, _, cx| {
                            weak.update(cx, |this, cx| {
                                this.open_member(author_token.clone(), author_name.clone());
                                cx.notify();
                            })
                            .ok();
                            cx.stop_propagation();
                        });
                    }
                    column = column.child(
                        div()
                            .id(SharedString::from(format!("q-answer-{}", answer.id)))
                            .w_full()
                            .flex_shrink_0()
                            .p_4()
                            .rounded_lg()
                            .bg(surface)
                            .border_1()
                            .border_color(border)
                            .flex()
                            .flex_col()
                            .gap_2()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_content(
                                    ContentKind::Answer,
                                    id.clone(),
                                    answer_title.clone(),
                                );
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(avatar)
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(muted)
                                            .child(SharedString::from(answer.author.name.clone())),
                                    )
                                    .child(div().flex_1())
                                    .child(div().text_xs().text_color(muted).child(
                                        SharedString::from(format!(
                                            "{} 赞同 · {} 评论",
                                            answer.vote_up_count, answer.comment_count
                                        )),
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(fg)
                                    .line_clamp(5)
                                    .child(SharedString::from(answer.excerpt.clone())),
                            ),
                    );
                }
            }
            if self.question_answers.is_empty() {
                // Keep the page usable while the first answer page loads.
            } else if self.question_end {
                column = column.child(
                    div()
                        .h(px(48.))
                        .flex_shrink_0()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(muted)
                        .child("没有更多回答"),
                );
            } else {
                column = column.child(
                    div()
                        .id("question-more")
                        .h(px(48.))
                        .flex_shrink_0()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_sm()
                        .text_color(accent)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_more_question_answers(cx);
                        }))
                        .child(if self.question_loading_more {
                            "加载中…"
                        } else {
                            "加载更多回答"
                        }),
                );
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(sort_row)
            .child(content)
            .into_any_element()
    }

    fn render_search(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let (query, member_scope) = match self.stack.last() {
            Some(Route::Search { query, member }) => (query.clone(), member.clone()),
            _ => (String::new(), None),
        };

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("search-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .h(px(38.))
                        .rounded_full()
                        .bg(bg)
                        .text_color(fg),
                ),
            );

        let mut type_row = div().flex().items_center().gap_2();
        for vertical in [
            SearchVertical::All,
            SearchVertical::Answer,
            SearchVertical::Article,
            SearchVertical::Video,
        ] {
            let active = self.search_vertical == vertical;
            type_row = type_row.child(
                div()
                    .id(SharedString::from(format!("search-v-{vertical:?}")))
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if active { accent } else { surface })
                    .text_xs()
                    .text_color(if active { rgb(0x0b0e11) } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.search_vertical != vertical {
                            this.search_vertical = vertical;
                            this.restart_search(cx);
                        }
                    }))
                    .child(vertical.label()),
            );
        }
        let mut sort_row = div().flex().items_center().gap_2();
        for sort in [
            SearchSort::Default,
            SearchSort::Latest,
            SearchSort::MostVoted,
        ] {
            let active = self.search_sort == sort;
            sort_row = sort_row.child(
                div()
                    .id(SharedString::from(format!("search-s-{sort:?}")))
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if active { surface } else { bg })
                    .border_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(if active { accent } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.search_sort != sort {
                            this.search_sort = sort;
                            this.restart_search(cx);
                        }
                    }))
                    .child(sort.label()),
            );
        }

        let query_empty = query.trim().is_empty();
        let content: AnyElement = if query_empty {
            let mut column = div()
                .id("search-hot-scroll")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col();
            if self.hot.is_empty() {
                column = column.child(empty_state("输入关键词开始搜索", muted));
            } else {
                column = column.child(
                    div()
                        .px_4()
                        .pt_3()
                        .pb_1()
                        .text_sm()
                        .text_color(muted)
                        .child("知乎热榜"),
                );
                for (index, item) in self.hot.iter().take(20).enumerate() {
                    let hot_query = item.query.clone();
                    column = column.child(
                        div()
                            .id(SharedString::from(format!("hot-search-{index}")))
                            .h(px(46.))
                            .w_full()
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .border_b_1()
                            .border_color(border)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.submit_search_query(hot_query.clone(), cx);
                            }))
                            .child(
                                div()
                                    .w(px(20.))
                                    .text_sm()
                                    .text_color(accent)
                                    .child(SharedString::from(format!("{}", index + 1))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(fg)
                                    .child(SharedString::from(item.query.clone())),
                            ),
                    );
                }
            }
            column.into_any_element()
        } else if !self.search_results.is_empty() {
            let entity = cx.entity().downgrade();
            let mut column = div()
                .id("search-results-scroll")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for item in &self.search_results {
                let read = self.is_read(item.kind, &item.id);
                column = column.child(feed_card(
                    item, &entity, surface, fg, muted, border, accent, 0.0, true, read,
                ));
            }
            if self.search_end {
                column = column.child(
                    div()
                        .id("search-end")
                        .h(px(48.))
                        .flex_shrink_0()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_xs().text_color(muted).child("没有更多结果")),
                );
            } else {
                let query_for_more = query.clone();
                let member_for_more = member_scope.clone();
                column = column.child(
                    div()
                        .id("search-more")
                        .h(px(48.))
                        .flex_shrink_0()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.search_loading_more {
                                return;
                            }
                            this.search_loading_more = true;
                            if let Some((member_hash, _)) = &member_for_more {
                                this.services.search_member(
                                    query_for_more.clone(),
                                    member_hash.clone(),
                                    this.search_offset,
                                    true,
                                    this.search_options(),
                                );
                            } else {
                                this.services.search(
                                    query_for_more.clone(),
                                    this.search_offset,
                                    true,
                                    this.search_options(),
                                );
                            }
                            cx.notify();
                        }))
                        .child(div().text_sm().text_color(accent).child(
                            if self.search_loading_more {
                                "加载中…"
                            } else {
                                "加载更多"
                            },
                        )),
                );
            }
            column.into_any_element()
        } else {
            let mut column = div().size_full().flex().flex_col();
            if !self.search_loading {
                if let Some(error) = self
                    .search_results_error
                    .as_ref()
                    .or(self.search_error.as_ref())
                {
                    column = column.child(
                        div()
                            .px_4()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(0xf6465d))
                            .child(SharedString::from(error.clone())),
                    );
                }
            }
            if self.search_suggestions.is_empty() {
                column = column.child(empty_state(
                    if self.search_loading {
                        "搜索中…"
                    } else {
                        "没有搜索结果"
                    },
                    muted,
                ));
            } else {
                let count = self.search_suggestions.len();
                column = column.child(
                    uniform_list(
                        "search-suggestions",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                            let mut rows = Vec::with_capacity(range.len());
                            for index in range {
                                rows.push(
                                    div()
                                        .id(SharedString::from(format!("suggest-{index}")))
                                        .h(px(52.))
                                        .w_full()
                                        .px_4()
                                        .flex()
                                        .items_center()
                                        .border_b_1()
                                        .border_color(border)
                                        .bg(surface)
                                        .text_base()
                                        .text_color(fg)
                                        .child(SharedString::from(
                                            this.search_suggestions[index].clone(),
                                        )),
                                );
                            }
                            rows
                        }),
                    )
                    .size_full(),
                );
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(self.render_search_history(cx))
            .child(if query_empty {
                div()
            } else {
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(type_row)
                    .child(sort_row)
            })
            .child(content)
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(surface)
                    .border_t_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("搜索建议 · 知乎公开接口"),
                    ),
            )
            .into_any_element()
    }

    fn render_search_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.search_history.is_empty() {
            return div().into_any_element();
        }
        let surface = if self.dark {
            rgb(0x181a20)
        } else {
            rgb(0xffffff)
        };
        let fg = if self.dark {
            rgb(0xeaecef)
        } else {
            rgb(0x1e2329)
        };
        let muted = if self.dark {
            rgb(0x848e9c)
        } else {
            rgb(0x6b7280)
        };
        let border = if self.dark {
            rgb(0x2b3139)
        } else {
            rgb(0xeaecef)
        };
        let mut row = div()
            .px_3()
            .pt_2()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(muted)
                    .child("最近搜索"),
            );
        for query in self.search_history.clone() {
            let action_query = query.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("search-history-{query}")))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(fg)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.search_from_history(action_query.clone(), cx);
                    }))
                    .child(SharedString::from(query)),
            );
        }
        row.child(
            div()
                .id("search-history-clear")
                .flex_shrink_0()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.services.clear_search_history();
                    this.search_history.clear();
                    cx.notify();
                }))
                .child("清空"),
        )
        .into_any_element()
    }

    fn render_tts(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = self
            .detail
            .as_ref()
            .map(|detail| detail.title.clone())
            .unwrap_or_default();
        let weak = cx.entity().downgrade();
        let count = self.tts_segments.len();
        let current = self.tts_index;
        let playing = self.tts_playing;

        let list = uniform_list(
            "tts-segments",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    let text = this.tts_segments.get(index).cloned().unwrap_or_default();
                    let active = index == this.tts_index;
                    let past = index < this.tts_index;
                    let weak = weak.clone();
                    rows.push(
                        div()
                            .id(("tts-segment", index))
                            .h(px(64.))
                            .w_full()
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .border_b_1()
                            .border_color(border)
                            .bg(if active { surface } else { bg })
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                weak.update(cx, |this, cx| {
                                    this.start_tts_at(index, cx);
                                })
                                .ok();
                            })
                            .child(
                                div()
                                    .w(px(22.))
                                    .text_xs()
                                    .text_color(if active { accent } else { muted })
                                    .child(SharedString::from(format!("{}", index + 1))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .line_height(px(22.))
                                    .line_clamp(2)
                                    .text_color(if active {
                                        accent
                                    } else if past {
                                        muted
                                    } else {
                                        fg
                                    })
                                    .child(SharedString::from(text)),
                            ),
                    );
                }
                rows
            }),
        )
        .flex_1()
        .min_h_0()
        .track_scroll(&self.tts_scroll);

        let bar_button = |id: &'static str, label: String, accent_text: bool| {
            div()
                .id(id)
                .px_4()
                .py_2()
                .rounded_full()
                .bg(if accent_text { accent } else { surface })
                .border_1()
                .border_color(if accent_text { accent } else { border })
                .text_sm()
                .text_color(if accent_text { rgb(0x0b0e11) } else { fg })
                .cursor_pointer()
                .child(SharedString::from(label))
        };

        let bar = div()
            .h(px(64.))
            .px_4()
            .flex()
            .items_center()
            .justify_center()
            .gap_3()
            .border_t_1()
            .border_color(border)
            .bg(surface)
            .child(
                bar_button("tts-prev", "上一句".to_string(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.advance_tts(-1, cx))),
            )
            .child(
                bar_button(
                    "tts-play",
                    if playing { "暂停" } else { "播放" }.to_string(),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_tts_pause(cx))),
            )
            .child(
                bar_button("tts-next", "下一句".to_string(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.advance_tts(1, cx))),
            )
            .child(
                bar_button("tts-exit", "退出朗读".to_string(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.exit_tts(cx))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("{}/{}", current + 1, count))),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.exit_tts(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h(px(56.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(border)
                    .bg(surface)
                    .child(
                        div()
                            .id("tts-exit-top")
                            .px_2()
                            .py_1()
                            .text_xl()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.exit_tts(cx)))
                            .child(
                                Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                                    .size(px(20.))
                                    .text_color(accent),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .truncate()
                            .child(SharedString::from(format!("朗读 · {title}"))),
                    ),
            )
            .child(list)
            .child(bar)
            .into_any_element()
    }

    fn render_annotation_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(quote) = self.selection_quote.clone() else {
            return div().into_any_element();
        };
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);

        let mut colors = div().flex().items_center().gap_2();
        for (key, label, color) in [
            ("yellow", "黄", rgb(0xe8c46a)),
            ("green", "绿", rgb(0x77b98b)),
            ("blue", "蓝", rgb(0x6fa8dc)),
            ("pink", "粉", rgb(0xd98aa3)),
        ] {
            let active = self.selection_color == key;
            colors = colors.child(
                div()
                    .id(SharedString::from(format!("highlight-color-{key}")))
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(color)
                    .border_1()
                    .border_color(if active { fg } else { color })
                    .text_xs()
                    .text_color(rgb(0x0b0e11))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selection_color = key.to_string();
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let preview: String = quote.chars().take(160).collect();
        div()
            .id("selection-annotation")
            .w_full()
            .flex_shrink_0()
            .px_4()
            .py_2()
            .bg(surface)
            .border_b_1()
            .border_color(border)
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("已复制 · 选中：{preview}"))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(colors)
                    .child(div().flex_1()),
            )
            .child(
                Input::new(&self.selection_note_input)
                    .h(px(36.))
                    .bg(bg)
                    .text_color(fg),
            )
            .child(
                Input::new(&self.selection_tags_input)
                    .h(px(36.))
                    .bg(bg)
                    .text_color(fg),
            )
            .children(self.notes_error.as_ref().map(|error| {
                div()
                    .text_xs()
                    .text_color(rgb(0xf6465d))
                    .child(SharedString::from(error.clone()))
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("highlight-save")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(accent)
                            .text_sm()
                            .text_color(rgb(0x0b0e11))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_selection_highlight(cx);
                            }))
                            .child("保存为笔记"),
                    )
                    .child(
                        div()
                            .id("highlight-cancel")
                            .px_3()
                            .py_1()
                            .text_sm()
                            .text_color(muted)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selection_quote = None;
                                cx.notify();
                            }))
                            .child("取消"),
                    ),
            )
            .into_any_element()
    }

    fn render_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let detail_id = self.stack.last().and_then(Route::id).map(str::to_string);
        if self.detail_selection_for != detail_id {
            self.detail_selection_for = detail_id;
            self.selection_quote = None;
        }
        if self.tts_active {
            return self.render_tts(cx);
        }
        self.scroll_to_pending_highlight();
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let typography = self.typography();
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = self
            .stack
            .last()
            .map(|route| route.title().to_string())
            .unwrap_or_default();
        let error = self.detail_error.clone();
        let annotation_panel = self.render_annotation_panel(cx);
        let header_question = self.detail.as_ref().and_then(|detail| {
            detail
                .question_id
                .as_ref()
                .map(|id| (id.clone(), detail.title.clone()))
        });
        let header_question_click = header_question.clone();

        let header = div()
            .min_h(px(56.))
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("detail-back")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .id("detail-question-title")
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .line_height(px(24.))
                    .font_weight(FontWeight::BOLD)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some((id, title)) = header_question_click.clone() {
                            this.open_question(id, title);
                            cx.notify();
                        }
                    }))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("detail-menu")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xl()
                    .text_color(if self.detail_menu_open { accent } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.detail_menu_open = !this.detail_menu_open;
                        cx.notify();
                    }))
                    .child("⋯"),
            );
        let menu = if self.detail_menu_open {
            self.render_detail_menu(header_question.clone(), cx)
        } else {
            div().into_any_element()
        };

        let mut image_urls = Vec::new();
        collect_image_urls(&self.detail_blocks, &mut image_urls);
        for url in image_urls {
            if self.image_errors.contains(&url) {
                continue;
            }
            if let Some(path) = self.services.ensure_image(&url) {
                self.image_paths.insert(url, path);
            }
        }

        let video_click: Option<render::VideoClickHandler> = {
            let weak = cx.entity().downgrade();
            Some(Arc::new(
                move |url: String, _window: &mut Window, cx: &mut gpui_kit::App| {
                    weak.update(cx, |this, cx| this.open_video(url, cx)).ok();
                },
            ))
        };
        let image_click: Option<render::ImageClickHandler> = {
            let weak = cx.entity().downgrade();
            Some(Arc::new(
                move |path: PathBuf, _window: &mut Window, cx: &mut gpui_kit::App| {
                    weak.update(cx, |this, cx| {
                        this.open_image(path);
                        cx.notify();
                    })
                    .ok();
                },
            ))
        };
        let copy_handler: Option<render::CopyHandler> = {
            let weak = cx.entity().downgrade();
            Some(Arc::new(
                move |text: String, _window: &mut Window, cx: &mut gpui_kit::App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    weak.update(cx, |this, cx| {
                        this.show_toast("已复制", cx);
                    })
                    .ok();
                },
            ))
        };
        let link_click: Option<render::LinkClickHandler> = {
            let weak = cx.entity().downgrade();
            Some(Arc::new(
                move |url: String, _window: &mut Window, cx: &mut gpui_kit::App| {
                    weak.update(cx, |this, cx| {
                        this.open_hyperlink(&url, cx);
                    })
                    .ok();
                },
            ))
        };
        let enhancements = render::ReadingEnhancements {
            first_line_indent: self.reading_indent,
            pos_colors: self.pos_colors,
            dark,
        };
        let actions = self.render_actions(cx);
        let body: AnyElement = if self.detail_loading {
            empty_state("加载中…", muted).into_any_element()
        } else if let Some(message) = error {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xdc2626))
                        .child(SharedString::from(message)),
                )
                .child(
                    div()
                        .id("detail-retry")
                        .px_4()
                        .py_2()
                        .rounded_lg()
                        .bg(accent)
                        .text_color(rgb(0x0b0e11))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.retry_detail(cx);
                            cx.notify();
                        }))
                        .child("重试"),
                )
                .into_any_element()
        } else if let Some(detail) = &self.detail {
            let meta =
                if let Some(time) = content_time_label(detail.created_time, detail.updated_time) {
                    format!("{} · {time}", detail.author.name)
                } else {
                    detail.author.name.clone()
                };
            let meta = if detail.author.is_mcn {
                format!("{meta} · MCN 机构")
            } else {
                meta
            };
            let author_token = detail
                .author
                .url_token
                .clone()
                .filter(|token| !token.is_empty())
                .unwrap_or_else(|| detail.author.id.clone());
            let author_name = detail.author.name.clone();
            let measure = typography.measure;
            div()
                .id("detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.detail_scroll)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        let text = TextSelection::selected_text(window, cx);
                        let text = text.trim().to_string();
                        if text.is_empty() || this.selection_quote.as_deref() == Some(text.as_str())
                        {
                            return;
                        }
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        this.selection_quote = Some(text);
                        this.notes_error = None;
                        this.show_toast("已复制选中内容", cx);
                    }),
                )
                .px_4()
                .py_3()
                .child(
                    div()
                        .w_full()
                        .max_w(px(measure))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("detail-author-meta")
                                .text_size(px(typography.caption_size + 1.0))
                                .line_height(px(typography.line_height * 0.8))
                                .text_color(muted)
                                .mb(px(typography.block_gap * 0.75))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !author_token.is_empty() {
                                        this.open_member(author_token.clone(), author_name.clone());
                                        cx.notify();
                                    }
                                }))
                                .child(SharedString::from(meta)),
                        )
                        .children(self.detail_offline.then(|| {
                            div()
                                .id("detail-offline-banner")
                                .mb(px(typography.block_gap))
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .bg(if dark { rgb(0x2b2410) } else { rgb(0xfff7e0) })
                                .border_1()
                                .border_color(accent)
                                .text_sm()
                                .text_color(if dark { rgb(0xf0b90b) } else { rgb(0x8a6400) })
                                .child("网络不可用，正在显示本地缓存的正文")
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(typography.block_gap))
                                .children(render_blocks(
                                    &self.detail_blocks,
                                    fg,
                                    muted,
                                    accent,
                                    surface,
                                    border,
                                    &typography,
                                    &self.image_paths,
                                    &self.image_errors,
                                    video_click.as_ref(),
                                    image_click.as_ref(),
                                    copy_handler.as_ref(),
                                    link_click.as_ref(),
                                    true,
                                    self.detail_highlight.as_ref().map(|highlight| {
                                        (highlight.quote.as_str(), note_color(&highlight.color))
                                    }),
                                    enhancements,
                                )),
                        ),
                )
                .into_any_element()
        } else {
            empty_state("内容不可用，请返回重试", muted).into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(menu)
            .child(annotation_panel)
            .child(body)
            .child(actions)
            .into_any_element()
    }

    fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("history-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("阅读历史"),
            )
            .child(if self.history.is_empty() {
                div().id("history-clear-empty")
            } else {
                div()
                    .id("history-clear")
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.clear_history();
                        this.history.clear();
                        cx.notify();
                    }))
                    .child("清空")
            });

        let content: AnyElement = if self.history.is_empty() {
            empty_state("暂无阅读记录", muted).into_any_element()
        } else {
            let mut column = div()
                .id("history-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_2();
            for entry in &self.history {
                let kind = match entry.kind.as_str() {
                    "article" => ContentKind::Article,
                    "pin" => ContentKind::Pin,
                    _ => ContentKind::Answer,
                };
                let id = entry.id.clone();
                let title = entry.title.clone();
                let kind_label = kind.label();
                column = column.child(
                    div()
                        .id(SharedString::from(format!("history-{}", entry.id)))
                        .w_full()
                        .p_3()
                        .rounded_lg()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .flex()
                        .flex_col()
                        .gap_1()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_content(kind, id.clone(), title.clone());
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(fg)
                                .truncate()
                                .child(SharedString::from(entry.title.clone())),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .text_xs()
                                .text_color(muted)
                                .child(kind_label)
                                .child(SharedString::from(format_comment_time(entry.read_at))),
                        ),
                );
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(content)
            .into_any_element()
    }

    fn render_diagnostics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("diag-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child("运行诊断"),
            )
            .child(
                div()
                    .id("diag-refresh")
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_diagnostics();
                        cx.notify();
                    }))
                    .child("刷新"),
            );

        let content: AnyElement = if let Some(diag) = &self.diag {
            let rows = vec![
                ("运行时长", format_uptime(diag.uptime)),
                ("数据目录", diag.data_dir.display().to_string()),
                ("本地数据", format_bytes(diag.total_bytes)),
                (
                    "图片缓存",
                    format!(
                        "{} 个 · {}",
                        diag.image_files,
                        format_bytes(diag.image_bytes)
                    ),
                ),
                ("崩溃日志", format!("{} 份", diag.crash_logs)),
                (
                    "推荐排序",
                    format!(
                        "{} · {} · {} 条本机反馈",
                        diag.recommend_mode.label(),
                        diag.recommend_preset.label(),
                        diag.recommend_feedback
                    ),
                ),
                (
                    "候选池",
                    format!(
                        "{} 条 · 向量 {} 条 · 当前来源 {}",
                        diag.recommend_pool_items,
                        diag.recommend_vectors,
                        diag.feed_source.label()
                    ),
                ),
                (
                    "推荐向量",
                    format!(
                        "{} · {} 条{}",
                        diag.recommend_model,
                        diag.recommend_vectors,
                        if diag.recommend_embedding_fallback {
                            "（统计回退）"
                        } else {
                            ""
                        }
                    ),
                ),
                (
                    "首屏推荐流",
                    diag.feed_ms
                        .map(|ms| format!("{ms} ms"))
                        .unwrap_or_else(|| "尚未加载".to_string()),
                ),
                (
                    "最近详情",
                    diag.detail_ms
                        .map(|ms| format!("{ms} ms"))
                        .unwrap_or_else(|| "尚未加载".to_string()),
                ),
            ];
            let mut column = div()
                .id("diag-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_2();
            for (label, value) in rows {
                column = column.child(
                    div()
                        .w_full()
                        .px_4()
                        .py_3()
                        .rounded_lg()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().text_sm().text_color(muted).child(label))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_right()
                                .truncate()
                                .text_sm()
                                .text_color(fg)
                                .child(SharedString::from(value)),
                        ),
                );
            }
            column.into_any_element()
        } else {
            empty_state("暂无诊断数据", muted).into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(content)
            .into_any_element()
    }

    fn render_notes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("notes-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("阅读笔记"),
            )
            .children((!self.highlights.is_empty()).then(|| {
                div()
                    .id("notes-clear")
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.clear_highlights();
                        this.highlights.clear();
                        cx.notify();
                    }))
                    .child("清空")
            }));
        let mut column = div()
            .id("notes-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3();
        if self.highlights.is_empty() {
            column = column.child(empty_state("暂无阅读笔记", muted));
        } else {
            for highlight in &self.highlights {
                let color = match highlight.color.as_str() {
                    "green" => rgb(0x77b98b),
                    "blue" => rgb(0x6fa8dc),
                    "pink" => rgb(0xd98aa3),
                    _ => rgb(0xe8c46a),
                };
                let id = highlight.id;
                let tags = highlight.tags.clone();
                let note = highlight.note.clone();
                let open_highlight = highlight.clone();
                column = column.child(
                    div()
                        .id(SharedString::from(format!("note-{id}")))
                        .w_full()
                        .flex_shrink_0()
                        .p_3()
                        .rounded_lg()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .border_l_3()
                        .border_color(color)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(fg)
                                .child(SharedString::from(highlight.title.clone())),
                        )
                        .child(
                            div()
                                .text_sm()
                                .line_height(px(22.))
                                .text_color(fg)
                                .child(SharedString::from(format!("“{}”", highlight.quote))),
                        )
                        .children((!note.is_empty()).then(|| {
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from(note))
                        }))
                        .children((!tags.is_empty()).then(|| {
                            div().flex().flex_wrap().gap_1().children(
                                tags.split(',')
                                    .filter(|tag| !tag.trim().is_empty())
                                    .map(|tag| {
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(bg)
                                            .text_xs()
                                            .text_color(accent)
                                            .child(SharedString::from(tag.trim().to_string()))
                                    }),
                            )
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("note-open-{id}")))
                                        .text_xs()
                                        .text_color(accent)
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_highlight_source(open_highlight.clone(), cx);
                                            cx.notify();
                                        }))
                                        .child("打开原文"),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("note-delete-{id}")))
                                        .text_xs()
                                        .text_color(muted)
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.services.delete_highlight(id);
                                            this.highlights = this.services.highlights(500);
                                            cx.notify();
                                        }))
                                        .child("删除"),
                                )
                                .child(div().flex_1())
                                .child(div().text_xs().text_color(muted).child(
                                    SharedString::from(format_comment_time(highlight.created_at)),
                                )),
                        ),
                );
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(column)
            .into_any_element()
    }

    fn render_crashes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let viewing = self.crash_view.is_some();

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("crashes-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(if viewing {
                        SharedString::from("崩溃日志详情")
                    } else {
                        SharedString::from(format!("崩溃日志 · {} 份", self.crash_logs.len()))
                    }),
            )
            .children((!viewing).then(|| {
                div()
                    .id("crashes-refresh")
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.crash_logs = crash::list_crashes();
                        this.crash_status =
                            Some(format!("共 {} 份崩溃日志", this.crash_logs.len()));
                        cx.notify();
                    }))
                    .child("刷新")
            }))
            .children((!viewing && !self.crash_logs.is_empty()).then(|| {
                div()
                    .id("crashes-clear")
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(0xf6465d))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let removed = crash::clear_crashes();
                        this.crash_logs = crash::list_crashes();
                        this.crash_status = Some(format!("已清空 {removed} 份日志"));
                        cx.notify();
                    }))
                    .child("清空")
            }));

        let content: AnyElement = if viewing {
            let text = self.crash_text.clone().unwrap_or_default();
            div()
                .id("crash-detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .child(
                    div()
                        .text_xs()
                        .text_color(fg)
                        .child(SharedString::from(text)),
                )
                .into_any_element()
        } else if self.crash_logs.is_empty() {
            empty_state("暂无崩溃日志\n应用崩溃时会自动保存到这里", muted).into_any_element()
        } else {
            let mut column = div()
                .id("crash-list-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_2();
            for (index, log) in self.crash_logs.clone().into_iter().enumerate() {
                let path = log.path.clone();
                column = column.child(
                    div()
                        .id(SharedString::from(format!("crash-{index}")))
                        .w_full()
                        .p_3()
                        .rounded_lg()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .flex()
                        .flex_col()
                        .gap_1()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crash_text = crash::read_crash(&path);
                            this.crash_view = Some(path.clone());
                            this.crash_status = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(fg)
                                .truncate()
                                .child(SharedString::from(log.summary)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from(crash::relative_time(log.modified)))
                                .child(SharedString::from(format!(
                                    "{:.1} KB",
                                    log.size as f64 / 1024.0
                                ))),
                        ),
                );
            }
            column.into_any_element()
        };

        let footer = div()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(border)
            .bg(surface)
            .text_xs()
            .text_color(if self.crash_status.is_some() {
                accent
            } else {
                muted
            })
            .child(SharedString::from(
                self.crash_status.clone().unwrap_or_else(|| {
                    "仅保存在本机，不会上传；日志不包含 Cookie 或正文".to_string()
                }),
            ));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(content)
            .child(footer)
            .into_any_element()
    }

    fn render_image_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (inset_top, _, inset_left, _) = safe_area();
        let close = div()
            .id("image-close")
            .absolute()
            .top(px(inset_top + 8.0))
            .left(px(inset_left + 12.0))
            .size(px(36.0))
            .rounded_full()
            .bg(rgb(0x1e2329))
            .flex()
            .items_center()
            .justify_center()
            .text_xl()
            .text_color(rgb(0xeaecef))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_image();
                cx.notify();
            }))
            .child("×");

        let zoom = self.image_zoom.clamp(1.0, 4.0);
        let picture: AnyElement = if let Some(path) = &self.image_view {
            let width = self.viewport_width.max(1.0);
            let height = self.viewport_height.max(1.0);
            div()
                .id("image-zoom-scroll")
                .size_full()
                .overflow_scroll()
                .track_scroll(&self.image_scroll)
                .child(
                    div()
                        .id("image-zoom-content")
                        .w(px(width * zoom))
                        .h(px(height * zoom))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            let now = std::time::Instant::now();
                            let double = this.image_last_tap.is_some_and(|last| {
                                now.duration_since(last) < std::time::Duration::from_millis(300)
                            });
                            this.image_last_tap = Some(now);
                            if double {
                                let zoomed_in = this.image_zoom > 1.0;
                                this.image_zoom = if zoomed_in { 1.0 } else { 2.5 };
                                if zoomed_in {
                                    this.image_scroll
                                        .set_offset(gpui_kit::point(px(0.), px(0.)));
                                } else {
                                    // Open on the middle of the picture, not its
                                    // top-left corner.
                                    let vw = this.viewport_width.max(1.0);
                                    let vh = this.viewport_height.max(1.0);
                                    let z = this.image_zoom;
                                    this.image_scroll.set_offset(gpui_kit::point(
                                        px(-vw * (z - 1.0) / 2.0),
                                        px(-vh * (z - 1.0) / 2.0),
                                    ));
                                }
                                this.image_last_tap = None;
                                cx.notify();
                            }
                        }))
                        .child(img(path.clone()).size_full().object_fit(ObjectFit::Contain)),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let total = self.image_view_list.len();
        let nav = |label: &'static str, forward: bool| {
            let mut button = div()
                .id(if forward { "image-next" } else { "image-prev" })
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .w(px(52.0))
                .flex()
                .items_center()
                .justify_center()
                .text_xl()
                .text_color(rgb(0xeaecef))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.step_image(forward);
                    cx.notify();
                }))
                .child(label);
            button = if forward {
                button.right(px(0.0))
            } else {
                button.left(px(0.0))
            };
            button
        };

        div()
            .size_full()
            .relative()
            .bg(rgb(0x000000))
            .capture_pinch(
                cx.listener(|this, event: &gpui_kit::PinchEvent, _window, cx| {
                    this.apply_image_pinch(
                        event.delta,
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                // Two-finger pinch also arrives as a pan; let the zoom win.
                if this
                    .image_pinch_at
                    .is_some_and(|last| last.elapsed() < std::time::Duration::from_millis(300))
                {
                    cx.stop_propagation();
                    return;
                }
                if event.modifiers.control {
                    let dy = match event.delta {
                        ScrollDelta::Pixels(delta) => f32::from(delta.y),
                        ScrollDelta::Lines(delta) => delta.y * 20.0,
                    };
                    if dy.abs() > 1.0 {
                        this.adjust_image_zoom(if dy > 0.0 { 0.25 } else { -0.25 }, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                let dx = match event.delta {
                    ScrollDelta::Pixels(delta) => f32::from(delta.x),
                    ScrollDelta::Lines(delta) => delta.x * 20.0,
                };
                // While zoomed in a horizontal drag pans the picture; only a
                // picture that fits the screen pages to the next one.
                if dx.abs() > 20.0 && this.image_zoom <= 1.01 {
                    this.step_image(dx < 0.0);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(picture)
            .child(
                div()
                    .absolute()
                    .bottom(px(inset_top + 52.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .id("image-zoom-out")
                            .size(px(34.))
                            .rounded_full()
                            .bg(rgb(0x1e2329))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_lg()
                            .text_color(rgb(0xeaecef))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adjust_image_zoom(-0.5, cx);
                            }))
                            .child("−"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x1e2329))
                            .text_xs()
                            .text_color(rgb(0xeaecef))
                            .child(SharedString::from(format!("{:.0}%", zoom * 100.0))),
                    )
                    .child(
                        div()
                            .id("image-zoom-in")
                            .size(px(34.))
                            .rounded_full()
                            .bg(rgb(0x1e2329))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_lg()
                            .text_color(rgb(0xeaecef))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adjust_image_zoom(0.5, cx);
                            }))
                            .child("＋"),
                    )
                    .child(
                        div()
                            .id("image-zoom-reset")
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x1e2329))
                            .text_xs()
                            .text_color(rgb(0xeaecef))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.image_zoom = 1.0;
                                this.image_scroll
                                    .set_offset(gpui_kit::point(px(0.), px(0.)));
                                cx.notify();
                            }))
                            .child("1:1"),
                    ),
            )
            .when(total > 1, |this| {
                this.child(nav("‹", false)).child(nav("›", true)).child(
                    div()
                        .absolute()
                        .bottom(px(inset_top + 16.0))
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(rgb(0x1e2329))
                                .text_xs()
                                .text_color(rgb(0xeaecef))
                                .child(SharedString::from(format!(
                                    "{} / {}",
                                    self.image_view_index + 1,
                                    total
                                ))),
                        ),
                )
            })
            .child(close)
            .into_any_element()
    }

    fn render_video(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("video-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_video();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_base()
                    .font_weight(FontWeight::BOLD)
                    .child("视频"),
            )
            .child(div().text_xs().text_color(muted).child("内嵌播放"));

        #[cfg(any(target_os = "android", target_os = "ios"))]
        let platform_surface: Option<Div> = self.video_handle.as_ref().map(|handle| {
            gpui_mobile::components::platform_view_element::platform_view_element(handle.clone())
        });
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let platform_surface: Option<Div> = None;

        let body: AnyElement = if let Some(surface) = platform_surface {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .bg(rgb(0x000000))
                .child(surface)
                .into_any_element()
        } else {
            let label = self
                .video_error
                .clone()
                .unwrap_or_else(|| "正在打开视频…".to_string());
            let color = if self.video_error.is_some() {
                rgb(0xf6465d)
            } else {
                muted
            };
            let mut column = div()
                .flex_1()
                .min_h_0()
                .p_6()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(color)
                        .child(SharedString::from(label)),
                );
            if let Some(url) = &self.video_url {
                column = column.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(SharedString::from(url.clone())),
                );
            }
            column.into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn show_toast(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.toast_message = Some(message.into());
        self.toast_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(2200));
        cx.notify();
    }

    fn tick_toast(&mut self, cx: &mut Context<Self>) {
        if self
            .toast_until
            .is_some_and(|until| std::time::Instant::now() >= until)
        {
            self.toast_message = None;
            self.toast_until = None;
            cx.notify();
        }
    }

    fn with_toast(&self, content: AnyElement) -> AnyElement {
        let Some(message) = &self.toast_message else {
            return content;
        };
        div()
            .size_full()
            .relative()
            .child(content)
            .child(
                div()
                    .id("app-toast")
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(84.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded_full()
                            .bg(rgb(0x1e2329))
                            .border_1()
                            .border_color(rgb(0x2b3139))
                            .shadow_lg()
                            .text_sm()
                            .text_color(rgb(0xeaecef))
                            .child(SharedString::from(format!("✓ {message}"))),
                    ),
            )
            .into_any_element()
    }

    fn tts_options(&self) -> tts::TtsOptions {
        tts::TtsOptions {
            rate: self.tts_rate,
            pitch: self.tts_pitch,
            volume: self.tts_volume,
            voice: self.tts_voice.clone(),
        }
    }

    fn tts_estimate_ms(&self, text: &str) -> u128 {
        let chars = text.chars().count() as f32;
        let chars_per_second = 4.5 * self.tts_rate.max(0.5);
        (chars / chars_per_second * 1000.0) as u128 + 500
    }

    fn start_tts(&mut self, cx: &mut Context<Self>) {
        if self.detail_blocks.is_empty() {
            return;
        }
        let segments = split_tts_segments(&self.detail_blocks);
        if segments.is_empty() {
            return;
        }
        self.tts_segments = segments;
        self.tts_index = 0;
        self.tts_active = true;
        self.tts_playing = true;
        self.tts_segment_started = std::time::Instant::now();
        self.speaking = true;
        if self.recommend_mode != RecommendMode::Off {
            self.record_detail_action("tts", 1.0);
        }
        self.speak_tts_index(cx);
    }

    fn speak_tts_index(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.tts_segments.get(self.tts_index).cloned() else {
            self.tts_playing = false;
            self.speaking = false;
            stop_speaking();
            cx.notify();
            return;
        };
        speak(&text, &self.tts_options());
        self.tts_segment_started = std::time::Instant::now();
        self.tts_scroll
            .scroll_to_item(self.tts_index, ScrollStrategy::Center);
        self.speaking = true;
        cx.notify();
    }

    fn start_tts_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tts_segments.len() {
            return;
        }
        self.tts_index = index;
        self.tts_playing = true;
        self.tts_segment_started = std::time::Instant::now();
        self.speak_tts_index(cx);
    }

    fn toggle_tts_pause(&mut self, cx: &mut Context<Self>) {
        if !self.tts_active {
            return;
        }
        if self.tts_playing {
            stop_speaking();
            self.tts_playing = false;
            self.speaking = false;
        } else {
            self.tts_playing = true;
            self.speak_tts_index(cx);
        }
        cx.notify();
    }

    fn advance_tts(&mut self, delta: i32, cx: &mut Context<Self>) {
        let last = self.tts_segments.len().saturating_sub(1) as i32;
        let next = (self.tts_index as i32 + delta).clamp(0, last) as usize;
        if next == self.tts_index && delta != 0 {
            return;
        }
        self.tts_index = next;
        self.tts_playing = true;
        self.tts_segment_started = std::time::Instant::now();
        self.speak_tts_index(cx);
    }

    fn tick_tts(&mut self, cx: &mut Context<Self>) {
        if !self.tts_active || !self.tts_playing {
            return;
        }
        let Some(text) = self.tts_segments.get(self.tts_index) else {
            return;
        };
        let estimate = self.tts_estimate_ms(text);
        let elapsed = self.tts_segment_started.elapsed().as_millis();
        let _ = is_speaking();
        if elapsed >= estimate {
            if self.tts_index + 1 >= self.tts_segments.len() {
                self.tts_playing = false;
                self.speaking = false;
                stop_speaking();
            } else {
                self.tts_index += 1;
                self.speak_tts_index(cx);
            }
            cx.notify();
        }
    }

    fn exit_tts(&mut self, cx: &mut Context<Self>) {
        stop_speaking();
        self.tts_active = false;
        self.tts_playing = false;
        self.speaking = false;
        self.tts_segments.clear();
        self.tts_index = 0;
        cx.notify();
    }

    fn copy_detail_markdown(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = &self.detail else {
            return;
        };
        let link = if let Some(id) = detail.id.strip_prefix("daily-") {
            format!("https://daily.zhihu.com/story/{id}")
        } else if detail.kind == ContentKind::Article {
            format!("https://zhuanlan.zhihu.com/p/{}", detail.id)
        } else {
            format!("https://www.zhihu.com/answer/{}", detail.id)
        };
        let body = zhihu_content::to_markdown(&self.detail_blocks);
        let text = format!(
            "# {}\n\n作者：{}\n\n{}\n\n原文：{}",
            detail.title, detail.author.name, body, link
        );
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.show_toast("已复制 Markdown", cx);
    }

    fn render_detail_menu(
        &self,
        header_question: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dark = self.dark;
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_3()
                .py_2()
                .rounded_md()
                .bg(surface)
                .border_1()
                .border_color(border)
                .text_sm()
                .text_color(fg)
                .cursor_pointer()
                .child(label)
        };
        let mut menu = div()
            .id("detail-menu-panel")
            .w_full()
            .px_4()
            .py_2()
            .flex()
            .flex_wrap()
            .gap_2()
            .bg(surface)
            .border_b_1()
            .border_color(border)
            .child(
                button("detail-menu-copy", "复制").on_click(cx.listener(|this, _, _, cx| {
                    this.detail_menu_open = false;
                    this.copy_detail_markdown(cx);
                    cx.notify();
                })),
            )
            .child(
                button(
                    "detail-menu-tts",
                    if self.speaking {
                        "停止朗读"
                    } else {
                        "朗读"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.detail_menu_open = false;
                    if this.tts_active {
                        this.exit_tts(cx);
                    } else {
                        this.start_tts(cx);
                    }
                    cx.notify();
                })),
            )
            .child(
                button("detail-menu-share", "分享").on_click(cx.listener(|this, _, _, cx| {
                    this.detail_menu_open = false;
                    if let Some(detail) = &this.detail {
                        let link = if let Some(id) = detail.id.strip_prefix("daily-") {
                            format!("https://daily.zhihu.com/story/{id}")
                        } else {
                            match detail.kind {
                                ContentKind::Answer => {
                                    format!("https://www.zhihu.com/answer/{}", detail.id)
                                }
                                ContentKind::Article => {
                                    format!("https://zhuanlan.zhihu.com/p/{}", detail.id)
                                }
                                ContentKind::Pin => {
                                    format!("https://www.zhihu.com/pin/{}", detail.id)
                                }
                                _ => format!("https://www.zhihu.com/answer/{}", detail.id),
                            }
                        };
                        let text = format!("{}\n{link}", detail.title);
                        #[cfg(any(target_os = "android", target_os = "ios"))]
                        {
                            let _ = &cx;
                            let _ = gpui_mobile::packages::share::share_text(&text, Some("知乎"));
                        }
                        #[cfg(not(any(target_os = "android", target_os = "ios")))]
                        {
                            let _ = text;
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(link));
                        }
                    }
                    cx.notify();
                })),
            )
            .child(
                button(
                    "detail-menu-follow",
                    if self.detail_following {
                        "取消关注"
                    } else {
                        "关注作者"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.detail_menu_open = false;
                    this.toggle_follow();
                    cx.notify();
                })),
            );
        if let Some((question_id, title)) = header_question {
            menu = menu.child(
                button("detail-menu-question", "全部回答").on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.detail_menu_open = false;
                        this.open_question(question_id.clone(), title.clone());
                        cx.notify();
                    },
                )),
            );
        }
        if self.recommend_mode != RecommendMode::Off {
            menu = menu
                .child(button("detail-menu-more", "更多类似").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.detail_menu_open = false;
                        this.record_explicit_preference("more", 2.0);
                        cx.notify();
                    },
                )))
                .child(button("detail-menu-less", "少看这类").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.detail_menu_open = false;
                        this.record_explicit_preference("less", -2.5);
                        cx.notify();
                    },
                )));
        }
        menu.child(
            button("detail-menu-block", "屏蔽作者").on_click(cx.listener(|this, _, _, cx| {
                this.detail_menu_open = false;
                let Some((id, name)) = this.detail.as_ref().map(|detail| {
                    let id = if detail.author.id.is_empty() {
                        detail.author.name.clone()
                    } else {
                        detail.author.id.clone()
                    };
                    (id, detail.author.name.clone())
                }) else {
                    return;
                };
                this.services.block_user(id, name);
                this.back();
                this.reload_sample_feed();
                this.loading_feed = true;
                this.services.refresh_feed();
                cx.notify();
            })),
        )
        .child(if let Some(error) = &self.action_error {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(0xdc2626))
                .child(SharedString::from(error.clone()))
        } else {
            div()
        })
        .into_any_element()
    }

    fn open_hyperlink(&mut self, raw: &str, cx: &mut Context<Self>) {
        let raw = raw.trim();
        if raw.is_empty() {
            return;
        }
        let Ok(url) = zhihu_content::Url::parse(raw) else {
            return;
        };
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if host == "link.zhihu.com" {
            if let Some((_, target)) = url.query_pairs().find(|(key, _)| key == "target") {
                self.open_hyperlink(&target, cx);
                return;
            }
        }
        if host == "zhihu.com" || host.ends_with(".zhihu.com") || host == "zhihu.cn" {
            if let Some(id) = topic_deeplink_target(raw) {
                self.open_topic(id, "话题".to_string());
                cx.notify();
                return;
            }
            if let Some((kind, id)) = deeplink_target(raw) {
                self.open_content(kind, id, "站内内容".to_string());
                cx.notify();
                return;
            }
            let segments: Vec<_> = url
                .path_segments()
                .map(|segments| segments.filter(|part| !part.is_empty()).collect())
                .unwrap_or_default();
            match segments.as_slice() {
                ["question", id] => {
                    self.open_question(id.to_string(), "问题".to_string());
                    cx.notify();
                    return;
                }
                ["people", token] => {
                    self.open_member(token.to_string(), "用户".to_string());
                    cx.notify();
                    return;
                }
                ["search"] => {
                    if let Some((_, query)) = url.query_pairs().find(|(key, _)| key == "q") {
                        self.open_search(query.to_string());
                        cx.notify();
                        return;
                    }
                }
                _ => {}
            }
        }
        open_external(raw);
        cx.notify();
    }

    fn render_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        if matches!(self.stack.last(), Some(Route::Daily { .. })) {
            return div().into_any_element();
        }
        let Some(detail) = &self.detail else {
            return div().into_any_element();
        };
        let dark = self.dark;
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let danger = rgb(0xf6465d);
        let vote_active = self.detail_vote == "up";
        let down_active = self.detail_vote == "down";

        div()
            .px_4()
            .py_2()
            .bg(surface)
            .border_t_1()
            .border_color(border)
            .flex()
            .items_center()
            .justify_around()
            .gap_2()
            .child(
                div()
                    .id("vote-up")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(fg)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vote_up();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(IconName::ThumbsUp)
                            .size(px(16.))
                            .text_color(if vote_active { accent } else { muted }),
                    )
                    .child(SharedString::from(format_count(detail.vote_up_count))),
            )
            .children((detail.kind != ContentKind::Pin).then(|| {
                div()
                    .id("vote-down")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(fg)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vote_down();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(IconName::ThumbsDown)
                            .size(px(16.))
                            .text_color(if down_active { danger } else { muted }),
                    )
            }))
            .children(
                matches!(detail.kind, ContentKind::Answer | ContentKind::Article).then(|| {
                    div()
                        .id("favorite-content")
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .bg(surface)
                        .border_1()
                        .border_color(border)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_favorite_picker(cx);
                        }))
                        .child(
                            Icon::new(gpui_kit::assets::IconName::Bookmark)
                                .size(px(16.))
                                .text_color(accent),
                        )
                }),
            )
            .child(
                div()
                    .id("open-comments")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some((kind, id, title)) = this
                            .detail
                            .as_ref()
                            .map(|detail| (detail.kind, detail.id.clone(), detail.title.clone()))
                        else {
                            return;
                        };
                        this.open_comments(kind, id, title);
                        cx.notify();
                    }))
                    .child(Icon::new(gpui_kit::assets::IconName::MessageCircle).size(px(16.)))
                    .child(SharedString::from(format_count(detail.comment_count))),
            )
            .child(if let Some(error) = &self.action_error {
                div()
                    .text_xs()
                    .text_color(rgb(0xdc2626))
                    .child(SharedString::from(error.clone()))
            } else {
                div()
            })
            .into_any_element()
    }
}

fn emoji_plain_text(text: &str, color: Rgba, size: f32, line_height: f32) -> AnyElement {
    use emoji::EmojiPiece;
    let pieces = emoji::pieces(text);
    if pieces.len() == 1 {
        if let EmojiPiece::Text(text) = &pieces[0] {
            return div()
                .text_size(px(size))
                .line_height(px(line_height))
                .text_color(color)
                .child(SharedString::from(text.clone()))
                .into_any_element();
        }
    }
    let mut row = div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .text_size(px(size))
        .line_height(px(line_height))
        .text_color(color);
    for piece in pieces {
        match piece {
            EmojiPiece::Emoji { path, placeholder } => {
                row = row.child(
                    img(path)
                        .size(px(size * 1.2))
                        .rounded_sm()
                        .aria_label(placeholder),
                );
            }
            EmojiPiece::Text(text) => {
                let mut chars = text.chars().peekable();
                let mut chunk = String::new();
                while let Some(ch) = chars.next() {
                    chunk.push(ch);
                    if chunk.chars().count() >= 20
                        || (chars.peek().is_none())
                        || ch.is_whitespace()
                        || matches!(ch, '，' | '。' | '！' | '？' | '；' | '：' | ',' | '.')
                    {
                        row = row.child(
                            div()
                                .text_size(px(size))
                                .line_height(px(line_height))
                                .text_color(color)
                                .child(SharedString::from(std::mem::take(&mut chunk))),
                        );
                    }
                }
            }
        }
    }
    row.into_any_element()
}

fn notification_card(
    notification: &zhihu_types::NotificationEntry,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
) -> AnyElement {
    let mut header = div().flex().items_center().gap_2();
    if !notification.is_read {
        header = header.child(div().size(px(8.)).rounded_full().bg(accent));
    }
    let mut avatar = div()
        .id(SharedString::from(format!(
            "notification-avatar-{}",
            notification.id
        )))
        .size(px(22.))
        .rounded_full()
        .bg(accent)
        .flex()
        .items_center()
        .justify_center();
    if let Some(url) = &notification.actor_avatar {
        avatar = avatar
            .bg(surface)
            .overflow_hidden()
            .child(img(url.clone()).size(px(22.)).rounded_full());
    } else {
        avatar = avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
    }
    let mut actor = div()
        .id(SharedString::from(format!(
            "notification-actor-{}",
            notification.id
        )))
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(SharedString::from(notification.actor.clone()));
    if let Some(token) = notification.actor_token.clone() {
        let weak_avatar = entity.clone();
        let token_avatar = token.clone();
        let name_avatar = notification.actor.clone();
        avatar = avatar.cursor_pointer().on_click(move |_, _, cx| {
            weak_avatar
                .update(cx, |this, cx| {
                    this.open_member(token_avatar.clone(), name_avatar.clone());
                    cx.notify();
                })
                .ok();
        });
        let weak_actor = entity.clone();
        let token_actor = token;
        let name_actor = notification.actor.clone();
        actor = actor.cursor_pointer().on_click(move |_, _, cx| {
            weak_actor
                .update(cx, |this, cx| {
                    this.open_member(token_actor.clone(), name_actor.clone());
                    cx.notify();
                })
                .ok();
        });
    }
    header = header.child(avatar).child(actor).child(div().flex_1());
    if notification.created > 0 {
        header = header.child(div().text_xs().text_color(muted).child(SharedString::from(
            format_comment_time(notification.created),
        )));
    }

    let click_notification = notification.clone();
    let click_weak = entity.clone();
    div()
        .id(SharedString::from(format!(
            "notification-card-{}",
            notification.id
        )))
        .w_full()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            click_weak
                .update(cx, |this, cx| {
                    this.open_notification(&click_notification, cx);
                    if let Some(entry) = this
                        .notifications
                        .iter_mut()
                        .find(|entry| entry.id == click_notification.id)
                    {
                        if !entry.is_read {
                            entry.is_read = true;
                            this.notification_unread = this.notification_unread.saturating_sub(1);
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .bg(surface)
        .border_1()
        .border_color(border)
        .rounded_lg()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(header)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(fg)
                .child(SharedString::from(notification.title.clone())),
        )
        .child(
            div()
                .text_sm()
                .line_height(px(22.))
                .text_color(muted)
                .child(SharedString::from(notification.text.clone())),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn comment_card(
    comment: &zhihu_types::Comment,
    weak: Option<&WeakEntity<ZhihuApp>>,
    expanded: bool,
    children: &[zhihu_types::Comment],
    children_loading: bool,
    children_end: bool,
    children_error: Option<&str>,
    has_replies: bool,
    allow_replies: bool,
    states: &HashMap<String, (i8, bool)>,
    highlight_id: Option<&str>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
) -> Div {
    let plain = zhihu_content::to_plain_text(&zhihu_content::parse_html(&comment.content).blocks);
    let time = format_comment_time(comment.created_time);
    let author_token = comment
        .author
        .url_token
        .clone()
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| comment.author.id.clone());
    let author_name = comment.author.name.clone();
    let mut avatar = div()
        .id(SharedString::from(format!("comment-avatar-{}", comment.id)))
        .size(px(22.))
        .rounded_full()
        .bg(accent)
        .flex()
        .items_center()
        .justify_center();
    if let Some(url) = &comment.author.avatar_url {
        avatar = avatar
            .bg(surface)
            .overflow_hidden()
            .child(img(url.clone()).size(px(22.)).rounded_full());
    } else {
        avatar = avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
    }
    let mut author = div()
        .id(SharedString::from(format!("comment-author-{}", comment.id)))
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(SharedString::from(author_name.clone()));
    if let Some(weak) = weak {
        if !author_token.is_empty() {
            let weak_avatar = weak.clone();
            let token_avatar = author_token.clone();
            let name_avatar = author_name.clone();
            avatar = avatar.cursor_pointer().on_click(move |_, _, cx| {
                weak_avatar
                    .update(cx, |this, cx| {
                        this.open_member(token_avatar.clone(), name_avatar.clone());
                        cx.notify();
                    })
                    .ok();
            });
            let weak_author = weak.clone();
            let token_author = author_token.clone();
            let name_author = author_name.clone();
            author = author.cursor_pointer().on_click(move |_, _, cx| {
                weak_author
                    .update(cx, |this, cx| {
                        this.open_member(token_author.clone(), name_author.clone());
                        cx.notify();
                    })
                    .ok();
            });
        }
    }
    let mut header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(avatar)
        .child(author);
    if comment.is_author {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(accent)
                .text_xs()
                .text_color(rgb(0x0b0e11))
                .child("作者"),
        );
    }
    if comment.author.is_mcn {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(0x3a2f1b))
                .text_xs()
                .text_color(rgb(0xf0b90b))
                .child("MCN"),
        );
    }
    if let Some(reply) = &comment.reply_to {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(0x1e3a5f))
                .text_xs()
                .text_color(rgb(0x8ec5ff))
                .child(SharedString::from(format!("回复 @{reply}"))),
        );
    }
    header = header.child(div().flex_1()).child(
        div()
            .text_xs()
            .text_color(muted)
            .child(SharedString::from(time)),
    );

    let (reaction, liked) = states.get(&comment.id).copied().unwrap_or((0, false));
    let highlighted = highlight_id == Some(comment.id.as_str());
    let like_count = comment.like_count + u64::from(liked);
    let mut footer = div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(muted);
    if let Some(weak) = weak {
        let weak_up = weak.clone();
        let id = comment.id.clone();
        footer = footer.child(
            div()
                .id(SharedString::from(format!("comment-up-{}", comment.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(surface)
                .text_color(muted)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_up
                        .update(cx, |this, cx| this.set_comment_reaction(&id, 1, cx))
                        .ok();
                })
                .child(
                    Icon::new(IconName::ThumbsUp)
                        .size(px(13.))
                        .text_color(if reaction == 1 { accent } else { muted }),
                ),
        );
        let weak_down = weak.clone();
        let id = comment.id.clone();
        footer = footer.child(
            div()
                .id(SharedString::from(format!("comment-down-{}", comment.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(surface)
                .text_color(muted)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_down
                        .update(cx, |this, cx| this.set_comment_reaction(&id, -1, cx))
                        .ok();
                })
                .child(
                    Icon::new(IconName::ThumbsDown)
                        .size(px(13.))
                        .text_color(if reaction == -1 { rgb(0xf6465d) } else { muted }),
                ),
        );
        let weak_like = weak.clone();
        let id = comment.id.clone();
        if !comment.is_mine {
            footer = footer.child(
                div()
                    .id(SharedString::from(format!("comment-like-{}", comment.id)))
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(surface)
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        weak_like
                            .update(cx, |this, cx| this.toggle_comment_like(&id, liked, cx))
                            .ok();
                    })
                    .child(
                        Icon::new(IconName::Heart)
                            .size(px(13.))
                            .text_color(if liked { accent } else { muted }),
                    )
                    .child(SharedString::from(format_count(like_count))),
            );
        } else {
            footer = footer.child(
                div()
                    .px_1()
                    .py_0p5()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(muted)
                    .child(Icon::new(IconName::Heart).size(px(13.)))
                    .child(SharedString::from(format_count(like_count))),
            );
        }
    } else {
        footer = footer
            .child(Icon::new(IconName::ThumbsUp).size(px(13.)))
            .child(SharedString::from(format_count(like_count)));
    }
    let copy_markdown =
        zhihu_content::to_markdown(&zhihu_content::parse_html(&comment.content).blocks);
    let copy_weak = weak.cloned();
    footer = footer.child(
        div()
            .id(SharedString::from(format!("comment-copy-{}", comment.id)))
            .text_xs()
            .text_color(accent)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_markdown.clone()));
                if let Some(weak) = &copy_weak {
                    weak.update(cx, |this, cx| {
                        this.show_toast("已复制评论", cx);
                    })
                    .ok();
                }
            })
            .child("复制"),
    );
    if let Some(weak) = weak {
        let weak_reply = weak.clone();
        let reply_id = comment.id.clone();
        let reply_name = author_name.clone();
        let root_id = comment.id.clone();
        footer = footer.child(
            div()
                .id(SharedString::from(format!("comment-reply-{}", comment.id)))
                .text_xs()
                .text_color(accent)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_reply
                        .update(cx, |this, cx| {
                            this.start_comment_reply(
                                reply_id.clone(),
                                reply_name.clone(),
                                root_id.clone(),
                                cx,
                            );
                        })
                        .ok();
                })
                .child("回复"),
        );
    }
    if allow_replies && (has_replies || expanded) {
        if let Some(weak) = weak {
            let id = comment.id.clone();
            let reply_count = comment.child_count.max(children.len() as u64);
            let label = if expanded {
                format!("▾ 收起回复（{reply_count}）")
            } else if reply_count > 0 {
                format!("▸ 共 {reply_count} 条回复")
            } else if children.is_empty() {
                "▸ 查看回复".to_string()
            } else {
                "▸ 展开回复".to_string()
            };
            let weak = weak.clone();
            footer = footer.child(
                div()
                    .id(SharedString::from(format!(
                        "comment-replies-{}",
                        comment.id
                    )))
                    .text_xs()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        weak.update(cx, |this, cx| {
                            if this.comment_children_open.contains(&id) {
                                this.comment_children_open.remove(&id);
                            } else {
                                this.comment_children_open.insert(id.clone());
                                if !this.comment_children.contains_key(&id) {
                                    this.comment_children_loading.insert(id.clone());
                                    this.services.load_child_comments(id.clone(), 0, false);
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .child(SharedString::from(label)),
            );
        }
    }

    let mut card = div()
        .w_full()
        .bg(surface)
        .border_1()
        .border_color(if highlighted { accent } else { border })
        .rounded_lg()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(header)
        .child(emoji_plain_text(&plain, fg, 14.0, 22.0))
        .child(footer);

    if expanded || !children.is_empty() {
        let mut column = div()
            .w_full()
            .mt_1()
            .pl_3()
            .border_l_1()
            .border_color(border)
            .flex()
            .flex_col()
            .gap_2();
        let shown = if expanded {
            children.len()
        } else {
            children.len().min(2)
        };
        let mut chain: Vec<String> = Vec::new();
        for (index, child) in children.iter().take(shown).enumerate() {
            let indent = child
                .reply_to
                .as_deref()
                .and_then(|target| chain.iter().rposition(|name| name == target))
                .map(|position| (position + 1).min(3))
                .unwrap_or(0);
            if chain.len() <= indent {
                chain.resize(indent + 1, String::new());
            } else {
                chain.truncate(indent + 1);
            }
            chain[indent] = child.author.name.clone();
            column = column.child(child_comment_row(
                child,
                weak,
                surface,
                fg,
                muted,
                accent,
                states,
                indent,
                index + 1,
                highlight_id == Some(child.id.as_str()),
                &comment.id,
            ));
        }
        if expanded {
            if children_loading {
                column = column.child(div().text_xs().text_color(muted).child("回复加载中…"));
            } else if let Some(error) = children_error {
                column = column.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xf6465d))
                        .child(SharedString::from(error.to_string())),
                );
            } else if children.is_empty() {
                column = column.child(div().text_xs().text_color(muted).child("暂无回复"));
            }
            if !children_end && !children_loading && !children.is_empty() {
                if let Some(weak) = weak {
                    let id = comment.id.clone();
                    let offset = children.len() as u32;
                    let weak = weak.clone();
                    column = column.child(
                        div()
                            .id(SharedString::from(format!("comment-more-{}", comment.id)))
                            .text_xs()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                weak.update(cx, |this, cx| {
                                    if this.comment_children_loading.insert(id.clone()) {
                                        this.services.load_child_comments(id.clone(), offset, true);
                                        cx.notify();
                                    }
                                })
                                .ok();
                            })
                            .child("加载更多回复"),
                    );
                }
            }
        }
        card = card.child(column);
    }
    card
}

#[allow(clippy::too_many_arguments)]
fn child_comment_row(
    child: &zhihu_types::Comment,
    weak: Option<&WeakEntity<ZhihuApp>>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    states: &HashMap<String, (i8, bool)>,
    indent: usize,
    floor: usize,
    highlighted: bool,
    root_id: &str,
) -> Div {
    let plain = zhihu_content::to_plain_text(&zhihu_content::parse_html(&child.content).blocks);
    let author_token = child
        .author
        .url_token
        .clone()
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| child.author.id.clone());
    let author_name = child.author.name.clone();
    let mut avatar = div()
        .id(SharedString::from(format!("child-avatar-{}", child.id)))
        .size(px(18.))
        .rounded_full()
        .bg(accent)
        .flex()
        .items_center()
        .justify_center();
    if let Some(url) = &child.author.avatar_url {
        avatar = avatar
            .bg(surface)
            .overflow_hidden()
            .child(img(url.clone()).size(px(18.)).rounded_full());
    } else {
        avatar = avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
    }
    let mut author = div()
        .id(SharedString::from(format!("child-author-{}", child.id)))
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(SharedString::from(author_name.clone()));
    if let Some(weak) = weak {
        if !author_token.is_empty() {
            let weak_name = weak.clone();
            let token = author_token.clone();
            let name = author_name.clone();
            author = author.cursor_pointer().on_click(move |_, _, cx| {
                weak_name
                    .update(cx, |this, cx| {
                        this.open_member(token.clone(), name.clone());
                        cx.notify();
                    })
                    .ok();
            });
            let weak_avatar = weak.clone();
            let token = author_token.clone();
            let name = author_name.clone();
            avatar = avatar.cursor_pointer().on_click(move |_, _, cx| {
                weak_avatar
                    .update(cx, |this, cx| {
                        this.open_member(token.clone(), name.clone());
                        cx.notify();
                    })
                    .ok();
            });
        }
    }
    let mut header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(avatar)
        .child(author);
    if child.is_author {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(accent)
                .text_xs()
                .text_color(rgb(0x0b0e11))
                .child("作者"),
        );
    }
    if child.author.is_mcn {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(0x3a2f1b))
                .text_xs()
                .text_color(rgb(0xf0b90b))
                .child("MCN"),
        );
    }
    if let Some(reply) = &child.reply_to {
        header = header.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(if child.is_author {
                    accent
                } else {
                    rgb(0x1f3b2e)
                })
                .text_xs()
                .text_color(if child.is_author {
                    rgb(0x0b0e11)
                } else {
                    rgb(0x7ee2a8)
                })
                .child(SharedString::from(format!("回复 @{reply}"))),
        );
    }
    let child_markdown =
        zhihu_content::to_markdown(&zhihu_content::parse_html(&child.content).blocks);
    let copy_weak = weak.cloned();
    header = header
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(SharedString::from(format!("#{floor}"))),
        )
        .child(div().flex_1())
        .child({
            if let Some(weak) = weak {
                let weak_reply = weak.clone();
                let reply_id = child.id.clone();
                let reply_name = author_name.clone();
                let reply_root = root_id.to_string();
                div()
                    .id(SharedString::from(format!("child-reply-{}", child.id)))
                    .text_xs()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        weak_reply
                            .update(cx, |this, cx| {
                                this.start_comment_reply(
                                    reply_id.clone(),
                                    reply_name.clone(),
                                    reply_root.clone(),
                                    cx,
                                );
                            })
                            .ok();
                    })
                    .child("回复")
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        })
        .child(
            div()
                .id(SharedString::from(format!("child-copy-{}", child.id)))
                .text_xs()
                .text_color(accent)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(child_markdown.clone()));
                    if let Some(weak) = &copy_weak {
                        weak.update(cx, |this, cx| {
                            this.show_toast("已复制回复", cx);
                        })
                        .ok();
                    }
                })
                .child("复制"),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(SharedString::from(format_comment_time(child.created_time))),
        );
    let (reaction, liked) = states.get(&child.id).copied().unwrap_or((0, false));
    let like_count = child.like_count + u64::from(liked);
    let mut actions = div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(muted);
    if let Some(weak) = weak {
        let weak_up = weak.clone();
        let id = child.id.clone();
        actions = actions.child(
            div()
                .id(SharedString::from(format!("child-up-{}", child.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(surface)
                .text_color(muted)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_up
                        .update(cx, |this, cx| this.set_comment_reaction(&id, 1, cx))
                        .ok();
                })
                .child(
                    Icon::new(IconName::ThumbsUp)
                        .size(px(13.))
                        .text_color(if reaction == 1 { accent } else { muted }),
                ),
        );
        let weak_down = weak.clone();
        let id = child.id.clone();
        actions = actions.child(
            div()
                .id(SharedString::from(format!("child-down-{}", child.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(surface)
                .text_color(muted)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_down
                        .update(cx, |this, cx| this.set_comment_reaction(&id, -1, cx))
                        .ok();
                })
                .child(
                    Icon::new(IconName::ThumbsDown)
                        .size(px(13.))
                        .text_color(if reaction == -1 { rgb(0xf6465d) } else { muted }),
                ),
        );
        let weak_like = weak.clone();
        let id = child.id.clone();
        if !child.is_mine {
            actions = actions.child(
                div()
                    .id(SharedString::from(format!("child-like-{}", child.id)))
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(surface)
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        weak_like
                            .update(cx, |this, cx| this.toggle_comment_like(&id, liked, cx))
                            .ok();
                    })
                    .child(
                        Icon::new(IconName::Heart)
                            .size(px(13.))
                            .text_color(if liked { accent } else { muted }),
                    )
                    .child(SharedString::from(format_count(like_count))),
            );
        } else {
            actions = actions.child(
                div()
                    .px_1()
                    .py_0p5()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(muted)
                    .child(Icon::new(IconName::Heart).size(px(13.)))
                    .child(SharedString::from(format_count(like_count))),
            );
        }
    } else {
        actions = actions
            .child(Icon::new(IconName::ThumbsUp).size(px(13.)))
            .child(SharedString::from(format_count(like_count)));
    }
    div()
        .w_full()
        .ml(px(indent as f32 * 14.0))
        .pl_2()
        .border_l_1()
        .border_color(if highlighted { accent } else { muted })
        .flex()
        .flex_col()
        .gap_1()
        .child(header)
        .child(emoji_plain_text(&plain, muted, 14.0, 20.0))
        .child(actions)
}

fn split_tts_segments(blocks: &[Block]) -> Vec<String> {
    let text = zhihu_content::to_plain_text(blocks);
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '；' | '\n') {
            let segment = current.trim();
            if !segment.is_empty() {
                segments.push(segment.to_string());
            }
            current.clear();
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        segments.push(tail.to_string());
    }
    segments
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn follow_index(kind: FollowFeedKind) -> usize {
    match kind {
        FollowFeedKind::Recommend => 0,
        FollowFeedKind::Activity => 1,
    }
}

fn note_color(key: &str) -> Rgba {
    match key {
        "green" => rgb(0x77b98b),
        "blue" => rgb(0x6fa8dc),
        "pink" => rgb(0xd98aa3),
        _ => rgb(0xe8c46a),
    }
}

/// Compact vote/comment counts: 999, 1.2k, 3.4w.
pub(crate) fn format_count(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }
    let (scaled, unit) = if value < 10_000 {
        (value as f64 / 1_000.0, "k")
    } else {
        (value as f64 / 10_000.0, "w")
    };
    let text = format!("{scaled:.1}");
    format!("{}{unit}", text.trim_end_matches(".0"))
}

fn action_kind(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Answer => "answer",
        ContentKind::Article => "article",
        ContentKind::Pin => "pin",
        ContentKind::Question => "question",
    }
}

fn dir_stats(root: &std::path::Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                files += 1;
                bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            }
        }
    }
    (files, bytes)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

fn format_uptime(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds} 秒")
    } else if seconds < 3_600 {
        format!("{} 分 {} 秒", seconds / 60, seconds % 60)
    } else {
        format!("{} 小时 {} 分", seconds / 3_600, (seconds % 3_600) / 60)
    }
}

fn content_time_label(created: i64, updated: i64) -> Option<String> {
    if updated > 0 && (created == 0 || updated > created + 60) {
        Some(format!("编辑于 {}", format_date_utc(updated)))
    } else if created > 0 {
        Some(format!("发布于 {}", format_date_utc(created)))
    } else {
        None
    }
}

/// UNIX seconds → `YYYY-MM-DD` (UTC). Avoids pulling a date crate for one
/// label; the day-level accuracy is enough for "published/edited".
fn format_date_utc(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_comment_time(created: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let diff = now.saturating_sub(created).max(0);
    if diff < 60 {
        "刚刚".to_string()
    } else if diff < 3_600 {
        format!("{} 分钟前", diff / 60)
    } else if diff < 86_400 {
        format!("{} 小时前", diff / 3_600)
    } else {
        format!("{} 天前", diff / 86_400)
    }
}

fn demo_comments() -> Vec<zhihu_types::Comment> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let author = |name: &str| Author {
        id: name.to_string(),
        name: name.to_string(),
        headline: None,
        url_token: None,
        is_following: false,
        avatar_url: None,
        is_mcn: false,
    };
    vec![
        zhihu_types::Comment {
            id: "demo-c1".to_string(),
            content: "<p>写得很好，尤其是把 UI 和核心逻辑解耦的部分。</p>".to_string(),
            author: author("读者甲"),
            reply_to: None,
            like_count: 12,
            child_count: 2,
            created_time: now - 1_800,
            is_author: false,
            is_mine: false,
            root_id: None,
            children: vec![zhihu_types::Comment {
                id: "demo-c1-1".to_string(),
                content: "<p>同感，尤其是输入法和生命周期那两段。</p>".to_string(),
                author: author("读者乙"),
                reply_to: Some("读者甲".to_string()),
                like_count: 2,
                child_count: 0,
                created_time: now - 1_200,
                is_author: false,
                is_mine: false,
                root_id: Some("demo-c1".to_string()),
                children: Vec::new(),
                child_next_offset: None,
            }],
            child_next_offset: None,
        },
        zhihu_types::Comment {
            id: "demo-c2".to_string(),
            content: "<p>补充一点：键盘遮挡和生命周期才是移动端最容易踩的坑。</p>".to_string(),
            author: author("示例作者"),
            reply_to: Some("读者甲".to_string()),
            like_count: 5,
            child_count: 0,
            created_time: now - 7_200,
            is_author: true,
            is_mine: false,
            root_id: None,
            children: Vec::new(),
            child_next_offset: None,
        },
    ]
}

#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
fn topic_deeplink_target(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("zhihurs://topic/") {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| id.to_string());
    }
    if let Some(rest) = url.split("/topic/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        return (!id.is_empty()).then(|| id.to_string());
    }
    None
}

#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
fn deeplink_target(url: &str) -> Option<(ContentKind, String)> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("zhihurs://") {
        let (kind, id) = rest.split_once('/')?;
        let id = id.split(['?', '#']).next()?.trim();
        if id.is_empty() {
            return None;
        }
        return match kind {
            "answer" => Some((ContentKind::Answer, id.to_string())),
            "article" | "p" => Some((ContentKind::Article, id.to_string())),
            "pin" => Some((ContentKind::Pin, id.to_string())),
            _ => None,
        };
    }
    if let Some(rest) = url.split("/answer/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        if !id.is_empty() {
            return Some((ContentKind::Answer, id.to_string()));
        }
    }
    if let Some(rest) = url.split("zhuanlan.zhihu.com/p/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        if !id.is_empty() {
            return Some((ContentKind::Article, id.to_string()));
        }
    }
    if let Some(rest) = url.split("/pin/").nth(1) {
        let id = rest.split(['?', '#', '/']).next()?.trim();
        if !id.is_empty() {
            return Some((ContentKind::Pin, id.to_string()));
        }
    }
    None
}

fn collect_image_urls(blocks: &[zhihu_content::Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            zhihu_content::Block::Image { src, .. } => out.push(src.clone()),
            zhihu_content::Block::Video { poster, .. } => {
                if !poster.is_empty() {
                    out.push(poster.clone());
                }
            }
            zhihu_content::Block::Quote(inner) => collect_image_urls(inner, out),
            _ => {}
        }
    }
}

fn qr_matrix_from_link(link: &str) -> Option<Vec<Vec<bool>>> {
    let code = qrcode::QrCode::new(link.as_bytes()).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    Some(
        (0..width)
            .map(|y| {
                (0..width)
                    .map(|x| colors[y * width + x] == qrcode::Color::Dark)
                    .collect()
            })
            .collect(),
    )
}

fn filter_sample_items(
    items: Vec<FeedItem>,
    blocklist: &[(String, bool)],
    blocked_users: &[(String, String)],
    blocked_topics: &[(String, String)],
) -> Vec<FeedItem> {
    items
        .into_iter()
        .filter(|item| {
            !blocklist.iter().any(|(pattern, _)| {
                item.title.contains(pattern)
                    || item.excerpt.contains(pattern)
                    || item.author.name.contains(pattern)
            }) && !blocked_users
                .iter()
                .any(|(id, name)| item.author.id == *id || item.author.name == *name)
                && !blocked_topics
                    .iter()
                    .any(|(id, name)| item.topics.iter().any(|topic| topic == id || topic == name))
        })
        .collect()
}

fn empty_state(message: &'static str, muted: Rgba) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(SharedString::from(message)),
        )
}

#[allow(clippy::too_many_arguments)]
fn hot_row(
    index: usize,
    item: &HotSearchItem,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
) -> impl gpui_kit::IntoElement {
    let rank_color = if index < 3 { accent } else { muted };
    let weak = entity.clone();
    let query = item.query.clone();
    div()
        .id(SharedString::from(format!("hot-{}", item.id)))
        .aria_label(item.query.clone())
        .h(px(56.))
        .w_full()
        .px_4()
        .flex()
        .items_center()
        .gap_3()
        .bg(surface)
        .border_b_1()
        .border_color(border)
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            weak.update(cx, |this, cx| {
                this.services.find_question(query.clone());
                cx.notify();
            })
            .ok();
        })
        .child(
            div()
                .w(px(22.))
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(rank_color)
                .child(SharedString::from(format!("{}", index + 1))),
        )
        .child(
            div()
                .flex_1()
                .text_base()
                .text_color(fg)
                .line_clamp(1)
                .child(SharedString::from(item.query.clone())),
        )
        .child(div().text_xs().text_color(muted).child(SharedString::from(
            item.hot_show.clone().unwrap_or_default(),
        )))
}

fn daily_row(
    item: &DailyStory,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
) -> impl gpui_kit::IntoElement {
    let id = item.id;
    let title = item.title.clone();
    let weak = entity.clone();
    div()
        .id(SharedString::from(format!("daily-{}", item.id)))
        .aria_label(item.title.clone())
        .h(px(96.))
        .w_full()
        .px_4()
        .py_2()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            weak.update(cx, |this, cx| {
                this.open_daily(id, title.clone());
                cx.notify();
            })
            .ok();
        })
        .child(
            div()
                .h_full()
                .w_full()
                .bg(surface)
                .border_1()
                .border_color(border)
                .rounded_lg()
                .p_3()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg)
                        .line_clamp(2)
                        .child(SharedString::from(item.title.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(SharedString::from(item.hint.clone())),
                ),
        )
}

#[allow(clippy::too_many_arguments)]
fn feed_card(
    item: &FeedItem,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
    offset: f32,
    wide: bool,
    read: bool,
) -> AnyElement {
    let danger = rgb(0xf6465d);
    let meta = format!(
        "{} 赞 · {} 评论",
        format_count(item.vote_up_count),
        format_count(item.comment_count)
    );
    let kind = item.kind;
    let id = item.id.clone();
    let title = item.title.clone();
    let has_topic = !item.topics.is_empty();

    let dislike = {
        let weak = entity.clone();
        let item_id = item.id.clone();
        div()
            .id(SharedString::from(format!("feed-dislike-{}", item.id)))
            .w(px(56.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(surface)
            .text_xs()
            .text_color(fg)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                weak.update(cx, |this, cx| this.dislike_feed_item(&item_id, cx))
                    .ok();
            })
            .child("不感兴趣")
    };
    let block_author = {
        let weak = entity.clone();
        let item_id = item.id.clone();
        div()
            .id(SharedString::from(format!("feed-block-author-{}", item.id)))
            .w(px(56.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(surface)
            .text_xs()
            .text_color(danger)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                weak.update(cx, |this, cx| this.block_feed_author(&item_id, cx))
                    .ok();
            })
            .child("屏蔽作者")
    };
    let block_topic = {
        let weak = entity.clone();
        let item_id = item.id.clone();
        let mut button = div()
            .id(SharedString::from(format!("feed-block-topic-{}", item.id)))
            .w(px(56.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(surface)
            .text_xs()
            .text_color(if has_topic { accent } else { muted })
            .child(if has_topic {
                "屏蔽话题"
            } else {
                "无话题"
            });
        if has_topic {
            button = button.cursor_pointer().on_click(move |_, _, cx| {
                weak.update(cx, |this, cx| this.block_feed_topic(&item_id, cx))
                    .ok();
            });
        }
        button
    };
    let actions = div()
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .w(px(168.))
        .flex()
        .overflow_hidden()
        .children([dislike, block_author, block_topic]);

    let author_token = item
        .author
        .url_token
        .clone()
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| item.author.id.clone());
    let author_name = item.author.name.clone();
    let mut avatar = div()
        .id(SharedString::from(format!("feed-avatar-{}", item.id)))
        .aria_label(author_name.clone())
        .size(px(24.))
        .rounded_full()
        .bg(accent)
        .flex()
        .items_center()
        .justify_center();
    if let Some(url) = &item.author.avatar_url {
        avatar = avatar
            .bg(surface)
            .overflow_hidden()
            .child(img(url.clone()).size(px(24.)).rounded_full());
    } else {
        avatar = avatar.child(div().text_xs().text_color(rgb(0x0b0e11)).child("知"));
    }
    if !author_token.is_empty() {
        let weak = entity.clone();
        let token = author_token.clone();
        let name = author_name.clone();
        avatar = avatar.cursor_pointer().on_click(move |_, _, cx| {
            weak.update(cx, |this, cx| {
                this.open_member(token.clone(), name.clone());
                cx.notify();
            })
            .ok();
            cx.stop_propagation();
        });
    }
    let mut author_label = div()
        .id(SharedString::from(format!("feed-author-{}", item.id)))
        .text_sm()
        .text_color(muted)
        .child(SharedString::from(author_name.clone()));
    if !author_token.is_empty() {
        let weak = entity.clone();
        let token = author_token.clone();
        let name = author_name.clone();
        author_label = author_label.cursor_pointer().on_click(move |_, _, cx| {
            weak.update(cx, |this, cx| {
                this.open_member(token.clone(), name.clone());
                cx.notify();
            })
            .ok();
            cx.stop_propagation();
        });
    }

    let click_weak = entity.clone();
    let down_weak = entity.clone();
    let swipe_id = item.id.clone();
    let mut topic_chips: Vec<AnyElement> = Vec::new();
    for topic in item.topic_refs.iter().take(3) {
        let weak = entity.clone();
        let topic_id = topic.id.clone();
        let topic_name = topic.name.clone();
        let mut chip = div()
            .id(SharedString::from(format!(
                "feed-topic-{}-{}",
                item.id, topic.name
            )))
            .px_2()
            .py_0p5()
            .rounded_full()
            .bg(surface)
            .border_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .child(SharedString::from(topic.name.clone()));
        if !topic_id.is_empty() {
            chip = chip.cursor_pointer().on_click(move |_, _, cx| {
                weak.update(cx, |this, cx| {
                    this.open_topic(topic_id.clone(), topic_name.clone());
                    cx.notify();
                })
                .ok();
                cx.stop_propagation();
            });
        }
        topic_chips.push(chip.into_any_element());
    }
    let mut content = div()
        .id(SharedString::from(format!("feed-body-{}", item.id)))
        .aria_label(item.title.clone())
        .w_full()
        .py_2()
        .ml(px(offset.clamp(0.0, 168.0)))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            down_weak
                .update(cx, |this, _| {
                    this.start_card_swipe(swipe_id.clone(), f32::from(event.position.x));
                })
                .ok();
        })
        .on_click(move |_, _, cx| {
            click_weak
                .update(cx, |this, cx| {
                    this.open_feed_item(kind, id.clone(), title.clone(), cx);
                })
                .ok();
        });
    if wide {
        content = content.px_4();
    }
    let content = content.child(
        div()
            .w_full()
            .bg(surface)
            .border_1()
            .border_color(border)
            .rounded_lg()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(avatar)
                    .child(author_label)
                    .children(item.author.is_mcn.then(|| {
                        div()
                            .px_1()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(0x3a2f1b))
                            .text_xs()
                            .text_color(rgb(0xf0b90b))
                            .child("MCN")
                    }))
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(0x2b3139))
                            .text_xs()
                            .text_color(rgb(0xf0b90b))
                            .child(item.kind.label()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(meta)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    // Read items stay in place but recede; unread titles keep
                    // full contrast so the feed still has a clear "new" order.
                    .text_color(if read { muted } else { fg })
                    .child(SharedString::from(item.title.clone())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .line_clamp(3)
                    .child(SharedString::from(item.excerpt.clone())),
            )
            .children(if topic_chips.is_empty() {
                Vec::new()
            } else {
                vec![div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(topic_chips)
                    .into_any_element()]
            }),
    );

    let mut wrapper = div()
        .id(SharedString::from(format!("feed-{}", item.id)))
        .w_full()
        .flex_shrink_0()
        .relative()
        .overflow_hidden();
    if offset > 0.0 {
        wrapper = wrapper.child(actions);
    }
    wrapper.child(content).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_zhihu_deeplinks() {
        assert_eq!(
            deeplink_target("https://www.zhihu.com/question/1/answer/123?share=1"),
            Some((ContentKind::Answer, "123".to_string()))
        );
        assert_eq!(
            deeplink_target("https://www.zhihu.com/answer/456"),
            Some((ContentKind::Answer, "456".to_string()))
        );
        assert_eq!(
            deeplink_target("https://zhuanlan.zhihu.com/p/789"),
            Some((ContentKind::Article, "789".to_string()))
        );
        assert_eq!(
            deeplink_target("zhihurs://answer/321"),
            Some((ContentKind::Answer, "321".to_string()))
        );
        assert_eq!(
            deeplink_target("zhihurs://article/654"),
            Some((ContentKind::Article, "654".to_string()))
        );
        assert_eq!(
            deeplink_target("https://www.zhihu.com/pin/101?share=1"),
            Some((ContentKind::Pin, "101".to_string()))
        );
        assert_eq!(
            deeplink_target("zhihurs://pin/102"),
            Some((ContentKind::Pin, "102".to_string()))
        );
        assert_eq!(
            topic_deeplink_target("https://www.zhihu.com/topic/19776749"),
            Some("19776749".to_string())
        );
        assert_eq!(
            topic_deeplink_target("zhihurs://topic/19776749?from=share"),
            Some("19776749".to_string())
        );
        assert_eq!(
            topic_deeplink_target("https://www.zhihu.com/question/1"),
            None
        );
        assert_eq!(deeplink_target("https://www.zhihu.com/question/1"), None);
        assert_eq!(deeplink_target("zhihurs://unknown/1"), None);
    }

    #[test]
    fn content_route_targets_survive_comment_back() {
        let answer = Route::Answer {
            id: "123".to_string(),
            title: "标题".to_string(),
        };
        assert_eq!(
            route_content_target(&answer),
            Some((ContentKind::Answer, "123".to_string()))
        );
        let article = Route::Article {
            id: "456".to_string(),
            title: "文章".to_string(),
        };
        assert_eq!(
            route_content_target(&article),
            Some((ContentKind::Article, "456".to_string()))
        );
        let pin = Route::Pin {
            id: "101".to_string(),
            title: "想法".to_string(),
        };
        assert_eq!(
            route_content_target(&pin),
            Some((ContentKind::Pin, "101".to_string()))
        );
        let comments = Route::Comments {
            kind: ContentKind::Answer,
            id: "123".to_string(),
            title: "标题".to_string(),
            anchor: None,
        };
        assert_eq!(route_content_target(&comments), None);
        assert_eq!(route_content_target(&Route::Notifications), None);
    }

    #[test]
    fn follow_feed_state_appends_and_deduplicates() {
        let mut state = FollowFeedState::default();
        let sample = m0_sample_feed();
        state.apply_page(
            vec![sample[0].clone(), sample[1].clone()],
            false,
            false,
            Some("next-1".to_string()),
        );
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.next.as_deref(), Some("next-1"));
        assert!(state.initialized);

        state.apply_page(vec![sample[1].clone(), sample[2].clone()], true, true, None);
        assert_eq!(state.items.len(), 3);
        assert!(state.is_end);
        assert!(state.next.is_none());
        assert_eq!(follow_index(FollowFeedKind::Activity), 1);
    }

    #[test]
    fn sorts_comments_by_time_and_likes() {
        let comment = |id: &str, created: i64, likes: u64| zhihu_types::Comment {
            id: id.to_string(),
            content: String::new(),
            author: Author {
                id: id.to_string(),
                name: id.to_string(),
                headline: None,
                url_token: None,
                is_following: false,
                avatar_url: None,
                is_mcn: false,
            },
            reply_to: None,
            like_count: likes,
            child_count: 0,
            created_time: created,
            is_author: false,
            is_mine: false,
            root_id: None,
            children: Vec::new(),
            child_next_offset: None,
        };
        let comments = vec![
            comment("old", 10, 1),
            comment("liked", 30, 9),
            comment("new", 20, 2),
        ];
        let ids = |sort| {
            sorted_comments(&comments, sort)
                .into_iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(CommentSort::Default), vec!["old", "liked", "new"]);
        assert_eq!(ids(CommentSort::Newest), vec!["liked", "new", "old"]);
        assert_eq!(ids(CommentSort::Oldest), vec!["old", "new", "liked"]);
        assert_eq!(ids(CommentSort::MostLiked), vec!["liked", "new", "old"]);
    }

    #[test]
    fn formats_content_dates() {
        assert_eq!(format_date_utc(0), "1970-01-01");
        assert_eq!(format_date_utc(1_709_164_800), "2024-02-29");
        assert_eq!(format_date_utc(1_793_145_600), "2026-10-28");
        assert_eq!(
            content_time_label(1_793_145_600, 1_793_145_600),
            Some("发布于 2026-10-28".to_string())
        );
        assert_eq!(
            content_time_label(1_709_164_800, 1_793_145_600),
            Some("编辑于 2026-10-28".to_string())
        );
        assert_eq!(content_time_label(0, 0), None);
    }

    #[test]
    fn formats_diagnostics_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
        let duration = |seconds| std::time::Duration::from_secs(seconds);
        assert_eq!(format_uptime(duration(45)), "45 秒");
        assert_eq!(format_uptime(duration(125)), "2 分 5 秒");
        assert_eq!(format_uptime(duration(3_720)), "1 小时 2 分");
    }
}
