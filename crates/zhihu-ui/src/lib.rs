//! GPUI views and components for the Zhihu client.
//!
//! M1 adds a real data path: the hot-search and Zhihu Daily tabs are backed by
//! public HTTP endpoints (no login required).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::{SelectionFormat, TextView, TextViewState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, img, px, rgb, uniform_list, AnyElement, ClipboardItem, Context, DispatchPhase, Div,
    Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, MouseMoveEvent,
    MouseUpEvent, ObjectFit, Render, Rgba, ScrollDelta, ScrollHandle, ScrollStrategy,
    ScrollWheelEvent, SharedString, TouchPhase, UniformListScrollHandle, WeakEntity, Window,
};
use zhihu_content::Block;
use zhihu_core::{AppServices, AppSettings, SearchOptions, SearchSort, SearchVertical, UiEvent};
use zhihu_recommend::{rerank, title_tokens, InterestProfile, RecommendMode};
use zhihu_types::{
    m0_sample_feed, AccountProfile, Author, ContentDetail, ContentKind, DailyStory, FeedItem,
    HotSearchItem,
};

mod crash;
mod home;
mod render;
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
pub(crate) const MONO_FONT: &str = "Noto Sans Mono CJK SC";
#[cfg(target_os = "ios")]
pub(crate) const MONO_FONT: &str = "Menlo";

const TABS: [&str; 5] = ["推荐", "关注", "热榜", "日报", "我的"];

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
    Search {
        query: String,
    },
    Comments {
        kind: ContentKind,
        id: String,
        title: String,
    },
    Question {
        id: String,
        title: String,
    },
    Daily {
        id: u64,
        title: String,
    },
    Member {
        token: String,
        name: String,
    },
    TtsSettings,
    Notifications,
    History,
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
struct DiagSnapshot {
    uptime: std::time::Duration,
    data_dir: PathBuf,
    total_bytes: u64,
    image_files: usize,
    image_bytes: u64,
    crash_logs: usize,
    recommend_mode: RecommendMode,
    recommend_feedback: usize,
    feed_ms: Option<u128>,
    detail_ms: Option<u128>,
}

impl Route {
    fn title(&self) -> &str {
        match self {
            Route::Answer { title, .. } | Route::Article { title, .. } | Route::Other { title } => {
                title
            }
            Route::Search { query } => query,
            Route::Comments { title, .. } => title,
            Route::Question { title, .. } => title,
            Route::Daily { title, .. } => title,
            Route::Member { name, .. } => name,
            Route::TtsSettings => "朗读设置",
            Route::Notifications => "消息",
            Route::History => "阅读历史",
            Route::Crashes => "崩溃日志",
            Route::Diagnostics => "运行诊断",
        }
    }

    fn id(&self) -> Option<&str> {
        match self {
            Route::Answer { id, .. } | Route::Article { id, .. } => Some(id),
            Route::Search { .. }
            | Route::Comments { .. }
            | Route::Question { .. }
            | Route::Daily { .. }
            | Route::Member { .. }
            | Route::TtsSettings
            | Route::Notifications
            | Route::History
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
    recommend_feedback_count: usize,
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
    detail_loading: bool,
    detail_error: Option<String>,
    speaking: bool,
    video_url: Option<String>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    video_player: Option<gpui_mobile::packages::video_player::VideoPlayer>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    video_handle: Option<Arc<gpui_mobile::platform_view::PlatformViewHandle>>,
    video_error: Option<String>,
    image_view: Option<PathBuf>,
    image_view_list: Vec<PathBuf>,
    image_view_index: usize,
    detail_vote: String,
    detail_following: bool,
    action_busy: bool,
    action_error: Option<String>,
    search_suggestions: Vec<String>,
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
    member: Option<zhihu_types::MemberProfile>,
    member_loading: bool,
    member_error: Option<String>,
    member_answers: Vec<FeedItem>,
    member_answers_offset: u32,
    member_answers_end: bool,
    member_answers_loading: bool,
    member_answers_error: Option<String>,
    member_following: bool,
    detail_select_mode: bool,
    detail_select_state: Option<Entity<TextViewState>>,
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
    notifications: Vec<zhihu_types::NotificationEntry>,
    notifications_loading: bool,
    notifications_error: Option<String>,
    notifications_offset: u32,
    notifications_end: bool,
    notifications_loading_more: bool,
    notifications_status: Option<String>,
    history: Vec<zhihu_types::HistoryEntry>,
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
    swipe_accum: f32,
    swipe_vertical: f32,
    swipe_decided: bool,
    swipe_latched: bool,
    swipe_latched_at: Option<std::time::Instant>,
    swipe_gesture_active: bool,
    list_epoch: u64,
    swipe_last_switch: Option<std::time::Instant>,
}

impl ZhihuApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索知乎"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let query = this.search.read(cx).value().trim().to_string();
                if !query.is_empty() {
                    this.open_search(query);
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
        let phone_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("手机号，例如 13800138000"));
        let sms_input = cx.new(|cx| InputState::new(window, cx).placeholder("6 位短信验证码"));
        let captcha_input = cx.new(|cx| InputState::new(window, cx).placeholder("图形验证码"));
        let services =
            Arc::new(AppServices::new(data_dir()).expect("failed to start network runtime"));
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
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let Some(this) = this.upgrade() else {
                break;
            };
            this.update(cx, |this, cx| {
                this.maybe_load_more_feed(cx);
                this.tick_toast(cx);
                this.tick_tts(cx);
            });
        })
        .detach();

        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            install_deeplink_handler();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
                let Some(this) = this.upgrade() else {
                    break;
                };
                let url = PENDING_DEEPLINK
                    .lock()
                    .ok()
                    .and_then(|mut slot| slot.take());
                let Some(url) = url else {
                    continue;
                };
                this.update(cx, |this, cx| {
                    if this.open_deeplink(&url) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }

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
        let recommend_feedback_count = services.recommend_feedback_count();
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
            recommend_feedback_count,
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
            detail_loading: false,
            detail_error: None,
            speaking: false,
            video_url: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            video_player: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            video_handle: None,
            video_error: None,
            image_view: None,
            image_view_list: Vec::new(),
            image_view_index: 0,
            detail_vote: "neutral".to_string(),
            detail_following: false,
            action_busy: false,
            action_error: None,
            search_suggestions: Vec::new(),
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
            member: None,
            member_loading: false,
            member_error: None,
            member_answers: Vec::new(),
            member_answers_offset: 0,
            member_answers_end: false,
            member_answers_loading: false,
            member_answers_error: None,
            member_following: false,
            detail_select_mode: false,
            detail_select_state: None,
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
            notifications: Vec::new(),
            notifications_loading: false,
            notifications_error: None,
            notifications_offset: 0,
            notifications_end: false,
            notifications_loading_more: false,
            notifications_status: None,
            history: Vec::new(),
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
            swipe_accum: 0.0,
            swipe_vertical: 0.0,
            swipe_decided: false,
            swipe_latched: false,
            swipe_latched_at: None,
            swipe_gesture_active: false,
            list_epoch: 0,
            swipe_last_switch: None,
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
                self.daily = items;
                self.daily_error = None;
                self.loading_daily = false;
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
                self.loading_feed = false;
                self.feed_loading_more = false;
                self.feed_error = None;
                self.error = None;
            }
            UiEvent::FeedMore { items, is_end } => {
                let empty = items.is_empty();
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
            UiEvent::Detail(detail) => {
                if let Some(requested) = self.detail_requested_at.take() {
                    self.detail_loaded_ms = Some(requested.elapsed().as_millis());
                }
                let matches_route = self
                    .stack
                    .last()
                    .and_then(Route::id)
                    .is_some_and(|id| id == detail.id);
                if matches_route {
                    self.detail_blocks = zhihu_content::parse_html(&detail.content_html).blocks;
                    self.detail = Some(detail);
                    self.detail_loading = false;
                    self.detail_error = None;
                }
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
                self.member = Some(member);
                self.member_loading = false;
                self.member_error = None;
            }
            UiEvent::MemberError(message) => {
                self.member_loading = false;
                self.member_error = Some(message);
            }
            UiEvent::MemberAnswers {
                items,
                append,
                is_end,
            } => {
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
                self.member_answers_loading = false;
                self.member_answers_error = Some(message);
            }
            UiEvent::MemberFollowed(following) => {
                self.member_following = following;
                self.member_error = None;
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
                items,
                append,
                is_end,
            } => {
                if append {
                    self.comments.extend(items);
                } else {
                    self.comments = items;
                }
                self.comments_offset = self.comments.len() as u32;
                self.comments_end = is_end;
                self.comments_loading = false;
                self.comments_loading_more = false;
                self.comments_error = None;
            }
            UiEvent::Notifications {
                items,
                append,
                is_end,
            } => {
                if append {
                    self.notifications.extend(items);
                } else {
                    self.notifications = items;
                }
                self.notifications_offset = self.notifications.len() as u32;
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
            UiEvent::NotificationsRead => {
                self.notifications_status = Some("已同步已读状态".to_string());
            }
            UiEvent::NotificationsReadError(message) => {
                self.notifications_status = Some(message);
            }
            UiEvent::CommentsError(message) => {
                self.comments_loading = false;
                self.comments_error = Some(message);
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
            UiEvent::QrRisk { message, redirect } => {
                self.qr_status = Some(message.clone());
                self.qr_risk = Some((message, redirect));
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
            recommend_feedback: self.recommend_feedback_count,
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

    fn open_comments(&mut self, kind: ContentKind, id: String, title: String) {
        self.stack.push(Route::Comments {
            kind,
            id: id.clone(),
            title,
        });
        self.comments.clear();
        self.comment_sort = CommentSort::Default;
        self.comments_error = None;
        self.comments_offset = 0;
        self.comments_end = false;
        self.comments_loading_more = false;
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

    fn search_options(&self) -> SearchOptions {
        SearchOptions {
            vertical: self.search_vertical,
            sort: self.search_sort,
        }
    }

    fn restart_search(&mut self, cx: &mut Context<Self>) {
        let query = match self.stack.last() {
            Some(Route::Search { query }) => query.clone(),
            _ => return,
        };
        self.search_results.clear();
        self.search_offset = 0;
        self.search_end = false;
        self.search_loading = true;
        self.search_loading_more = false;
        self.search_error = None;
        self.search_results_error = None;
        self.services.search(query, 0, false, self.search_options());
        cx.notify();
    }

    fn open_search(&mut self, query: String) {
        self.stack.push(Route::Search {
            query: query.clone(),
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
            typography: self.typography_preset.as_str().to_string(),
        });
    }

    /// Rebuild the displayed feed from the server order plus local interest.
    fn apply_recommendation(&mut self) {
        if self.feed_raw.is_empty() {
            self.feed.clear();
            return;
        }
        let mut items = self.feed_raw.clone();
        let disliked: HashSet<String> = self.services.disliked_item_ids().into_iter().collect();
        if !disliked.is_empty() {
            items.retain(|item| {
                !disliked.contains(&format!("{}:{}", action_kind(item.kind), item.id))
            });
        }
        if self.recommend_mode != RecommendMode::Off {
            let profile =
                InterestProfile::from_feedback(&self.services.recommend_feedback(2000), unix_now());
            rerank(&mut items, &profile, self.recommend_mode);
        }
        self.feed = items;
    }

    pub(crate) fn set_recommend_mode(&mut self, mode: RecommendMode, cx: &mut Context<Self>) {
        self.recommend_mode = mode;
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
        self.card_swipe_id = None;
        self.card_swipe_open = None;
        self.card_swipe_dx = 0.0;
        cx.notify();
    }

    fn load_more_feed(&mut self, cx: &mut Context<Self>) {
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
        let offset = f32::from(self.feed_scroll.offset().y);
        let max = f32::from(self.feed_scroll.max_offset().y);
        let viewport = f32::from(self.feed_scroll.bounds().size.height);
        // With two columns a short page may not overflow at all; keep fetching
        // until the viewport is filled.
        if max > 0.0 && offset + viewport < max - 240.0 {
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
            0 | 1 => {
                if self.services.is_logged_in() && self.feed.is_empty() && !self.loading_feed {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                }
            }
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
    /// reaches the list and cannot scroll it vertically. Returns `true` when
    /// the event was consumed.
    fn handle_horizontal_swipe(&mut self, event: &ScrollWheelEvent) -> bool {
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(delta) => (delta.x * 20.0, delta.y * 20.0),
        };
        let ended = matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
        if event.touch_phase == TouchPhase::Started {
            self.swipe_accum = 0.0;
            self.swipe_vertical = 0.0;
            self.swipe_decided = false;
            self.swipe_latched = false;
            self.swipe_latched_at = None;
            self.swipe_gesture_active = true;
        }
        if ended {
            self.swipe_gesture_active = false;
        }

        if self.swipe_latched {
            // Touch gestures always deliver Started/Ended, but a desktop
            // mouse wheel arrives as a stream of `Moved` events with no end
            // marker. Expire the latch on a timer in that case, otherwise
            // the first wheel notch would swallow every later one.
            let stale = !self.swipe_gesture_active
                && self
                    .swipe_latched_at
                    .is_some_and(|at| at.elapsed().as_millis() >= 400);
            if ended || stale {
                self.swipe_accum = 0.0;
                self.swipe_vertical = 0.0;
                self.swipe_decided = false;
                self.swipe_latched = false;
                self.swipe_latched_at = None;
                if ended {
                    return true;
                }
            } else {
                return true;
            }
        }
        if self.swipe_decided {
            if ended {
                self.swipe_accum = 0.0;
                self.swipe_vertical = 0.0;
                self.swipe_decided = false;
            }
            return false;
        }

        if !self.swipe_gesture_active {
            // A desktop mouse wheel has no Started/Ended markers: every
            // notch is an independent gesture, so do not mix its deltas
            // with the previous notch.
            self.swipe_accum = 0.0;
            self.swipe_vertical = 0.0;
        }

        self.swipe_accum += dx;
        self.swipe_vertical += dy;
        let horizontal = self.swipe_accum.abs();
        let vertical = self.swipe_vertical.abs();
        if horizontal >= 24.0 && horizontal > vertical * 1.25 {
            self.swipe_decided = true;
            self.swipe_latched = true;
            self.swipe_latched_at = Some(std::time::Instant::now());
            if self
                .swipe_last_switch
                .is_none_or(|last| last.elapsed().as_millis() >= 350)
            {
                let next = if self.swipe_accum < 0.0 {
                    (self.tab + 1).min(TABS.len() - 1)
                } else {
                    self.tab.saturating_sub(1)
                };
                if next != self.tab {
                    self.swipe_last_switch = Some(std::time::Instant::now());
                    self.select_tab(next);
                }
            }
            return true;
        }
        if vertical >= 16.0 && vertical > horizontal {
            if self.swipe_gesture_active {
                self.swipe_decided = true;
            }
            self.swipe_accum = 0.0;
        }
        if ended {
            self.swipe_accum = 0.0;
            self.swipe_vertical = 0.0;
            self.swipe_decided = false;
        }
        false
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
                            let before = this.tab;
                            let consumed = this.handle_horizontal_swipe(event);
                            if this.tab != before {
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
        self.detail_select_mode = false;
        self.detail_select_state = None;
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
            _ => Route::Other {
                title: title.clone(),
            },
        };
        self.stack.push(route);
        self.detail = None;
        self.detail_blocks.clear();
        self.detail_error = None;
        self.detail_loading = false;
        self.image_errors.clear();
        self.detail_vote = "neutral".to_string();
        self.detail_following = false;
        self.action_error = None;

        if id.starts_with("m0-") || matches!(kind, ContentKind::Pin) {
            self.show_demo(title, kind);
        } else if !self.services.is_logged_in() {
            self.detail_error = Some("需要登录 Cookie 后才能加载正文".to_string());
        } else {
            self.detail_loading = true;
            self.detail_requested_at = Some(std::time::Instant::now());
            self.services.load_detail(kind, id);
        }
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
        self.services.load_question(id.clone());
        self.services
            .load_question_answers(id, 0, false, "default".to_string());
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
        self.member_answers.clear();
        self.member_answers_offset = 0;
        self.member_answers_end = false;
        self.member_answers_loading = false;
        self.member_answers_error = None;
        self.member_following = false;
        self.services.load_member(token.clone());
        self.services.load_member_answers(token, 0, false);
    }

    fn load_more_member_answers(&mut self, cx: &mut Context<Self>) {
        if self.member_answers_end || self.member_answers_loading {
            return;
        }
        let token = match self.stack.last() {
            Some(Route::Member { token, .. }) => token.clone(),
            _ => return,
        };
        self.member_answers_loading = true;
        self.services
            .load_member_answers(token, self.member_answers_offset, true);
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
    }

    fn close_image(&mut self) {
        self.image_view = None;
        self.image_view_list.clear();
        self.image_view_index = 0;
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
        self.detail_select_mode = false;
        self.detail_select_state = None;
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
            0 | 1 => {
                self.feed_error = None;
                if self.services.is_logged_in() {
                    self.loading_feed = true;
                    self.services.refresh_feed();
                } else {
                    self.reload_sample_feed();
                    self.loading_feed = false;
                }
            }
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
            Route::Search { query } => {
                self.search_loading = true;
                self.search_error = None;
                self.services.search_suggest(query);
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
            Route::TtsSettings => {}
            Route::History => {
                self.history = self.services.history(200);
            }
            Route::Crashes => {
                self.crash_logs = crash::list_crashes();
                self.crash_view = None;
                self.crash_text = None;
            }
            Route::Diagnostics => {
                self.refresh_diagnostics();
            }
            Route::Comments { kind, id, title } => {
                if id == "demo" {
                    self.comments = demo_comments();
                    self.comments_loading = false;
                    self.comments_error = None;
                    self.comments_end = true;
                } else {
                    self.comments = Vec::new();
                    self.comments_offset = 0;
                    self.comments_end = false;
                    self.comments_loading = true;
                    self.comments_error = None;
                    self.services.load_comments(kind, id, 0, false);
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
        let content: AnyElement = if self.image_view.is_some() {
            self.render_image_view(cx)
        } else if self.video_url.is_some() {
            self.render_video(cx)
        } else if !self.stack.is_empty() {
            let page = if matches!(self.stack.last(), Some(Route::Search { .. })) {
                self.render_search(cx)
            } else if matches!(self.stack.last(), Some(Route::Comments { .. })) {
                self.render_comments(cx)
            } else if matches!(self.stack.last(), Some(Route::Question { .. })) {
                self.render_question(cx)
            } else if matches!(self.stack.last(), Some(Route::Member { .. })) {
                self.render_member(cx)
            } else if matches!(self.stack.last(), Some(Route::TtsSettings)) {
                self.render_tts_settings(cx)
            } else if matches!(self.stack.last(), Some(Route::Notifications)) {
                self.render_notifications(cx)
            } else if matches!(self.stack.last(), Some(Route::History)) {
                self.render_history(cx)
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
        self.with_toast(content)
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
                    .child("←"),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("消息"),
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
            let cards: Vec<Div> = self
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

    fn render_comments(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
                    .child("←"),
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
                    let expanded = self.comment_children_open.contains(&comment.id)
                        || !comment.children.is_empty();
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
                        &states,
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
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3();
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
                    .child("←"),
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
                    .child("←"),
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
                    column = column.child(empty_state("暂无回答", muted));
                }
            } else {
                for answer in &self.member_answers {
                    let id = answer.id.clone();
                    let answer_title = answer.title.clone();
                    column = column.child(
                        div()
                            .id(SharedString::from(format!("member-answer-{}", answer.id)))
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
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(fg)
                                    .line_clamp(2)
                                    .child(SharedString::from(answer.title.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(SharedString::from(format!(
                                        "{} 赞同 · {} 评论",
                                        answer.vote_up_count, answer.comment_count
                                    ))),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .line_clamp(4)
                                    .child(SharedString::from(answer.excerpt.clone())),
                            ),
                    );
                }
            }
            if !self.member_answers.is_empty() {
                if self.member_answers_end {
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
                            .child(if self.member_answers_loading {
                                "加载中…"
                            } else {
                                "加载更多回答"
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
                    .child("←"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(title.clone())),
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

        let content: AnyElement =
            if self.question_loading && self.question.is_none() {
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
                    let mut card = div()
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
                        column =
                            column.child(
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
                                            .child(
                                                div()
                                                    .size(px(24.))
                                                    .rounded_full()
                                                    .bg(accent)
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(rgb(0x0b0e11))
                                                            .child("知"),
                                                    ),
                                            )
                                            .child(div().text_sm().text_color(muted).child(
                                                SharedString::from(answer.author.name.clone()),
                                            ))
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
        let query = match self.stack.last() {
            Some(Route::Search { query }) => query.clone(),
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
                    .child("←"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(format!("搜索：{query}"))),
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

        let content: AnyElement = if !self.search_results.is_empty() {
            let entity = cx.entity().downgrade();
            let mut column = div()
                .id("search-results-scroll")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for item in &self.search_results {
                column = column.child(feed_card(
                    item, &entity, surface, fg, muted, border, accent, 0.0,
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
                            this.services.search(
                                query_for_more.clone(),
                                this.search_offset,
                                true,
                                this.search_options(),
                            );
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
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(type_row)
                    .child(sort_row),
            )
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
                            .child("←"),
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

    fn render_detail_selection(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let (title, html) = self
            .detail
            .as_ref()
            .map(|detail| (detail.title.clone(), detail.content_html.clone()))
            .unwrap_or_default();

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
                    this.detail_select_mode = false;
                    cx.stop_propagation();
                    cx.notify();
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
                            .id("selection-exit")
                            .px_2()
                            .py_1()
                            .text_xl()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detail_select_mode = false;
                                cx.notify();
                            }))
                            .child("←"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .truncate()
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("松开左键自动复制选中内容"),
                    ),
            )
            .child({
                let mut body = div()
                    .id("detail-selection-body")
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            let Some(state) = &this.detail_select_state else {
                                return;
                            };
                            let text = state.read(cx).selected_text();
                            if text.trim().is_empty() {
                                return;
                            }
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            this.show_toast("已复制选中内容", cx);
                        }),
                    );
                if let Some(state) = &self.detail_select_state {
                    body = body.child(
                        TextView::new(state)
                            .selectable(true)
                            .selection_format(SelectionFormat::Source)
                            .scrollable(true)
                            .size_full(),
                    );
                } else {
                    body = body.child(
                        TextView::html("detail-selectable-text", html)
                            .selectable(true)
                            .selection_format(SelectionFormat::Source)
                            .scrollable(true)
                            .size_full(),
                    );
                }
                body
            })
            .into_any_element()
    }

    fn render_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.tts_active {
            return self.render_tts(cx);
        }
        if self.detail_select_mode {
            return self.render_detail_selection(cx);
        }
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
        let header_question = self.detail.as_ref().and_then(|detail| {
            detail
                .question_id
                .as_ref()
                .map(|id| (id.clone(), detail.title.clone()))
        });

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
                    .id("detail-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child("←"),
            )
            .child(
                div()
                    .id("detail-question-title")
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some((id, title)) = header_question.clone() {
                            this.open_question(id, title);
                            cx.notify();
                        }
                    }))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("detail-copy")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.copy_detail_markdown(cx);
                    }))
                    .child("复制"),
            )
            .child(
                div()
                    .id("detail-select")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let html = this
                            .detail
                            .as_ref()
                            .map(|detail| detail.content_html.clone())
                            .unwrap_or_default();
                        this.detail_select_state = Some(cx.new(|cx| {
                            TextViewState::html(&html, cx)
                                .selectable(true)
                                .selection_format(SelectionFormat::Source)
                        }));
                        this.detail_select_mode = true;
                        cx.notify();
                    }))
                    .child("选择"),
            )
            .child(
                div()
                    .id("detail-share")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
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
                                    _ => format!("https://www.zhihu.com/answer/{}", detail.id),
                                }
                            };
                            let text = format!(
                                "{}
{link}",
                                detail.title
                            );
                            #[cfg(any(target_os = "android", target_os = "ios"))]
                            {
                                let _ = &cx;
                                let _ =
                                    gpui_mobile::packages::share::share_text(&text, Some("知乎"));
                            }
                            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                            {
                                let _ = text;
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(link));
                            }
                        }
                    }))
                    .child("分享"),
            )
            .child(
                div()
                    .id("detail-tts")
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.tts_active {
                            this.exit_tts(cx);
                        } else {
                            this.start_tts(cx);
                        }
                        cx.notify();
                    }))
                    .child(if self.speaking { "停止" } else { "朗读" }),
            );

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
            .child(actions)
            .child(body)
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
                    .child("←"),
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
                let kind = if entry.kind == "article" {
                    ContentKind::Article
                } else {
                    ContentKind::Answer
                };
                let id = entry.id.clone();
                let title = entry.title.clone();
                let kind_label = if kind == ContentKind::Article {
                    "文章"
                } else {
                    "回答"
                };
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
                    .child("←"),
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
                        "{} · {} 条本机反馈",
                        diag.recommend_mode.label(),
                        diag.recommend_feedback
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
                    .child("←"),
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

        let picture: AnyElement = if let Some(path) = &self.image_view {
            img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
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
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let dx = match event.delta {
                    ScrollDelta::Pixels(delta) => f32::from(delta.x),
                    ScrollDelta::Lines(delta) => delta.x * 20.0,
                };
                if dx.abs() > 20.0 {
                    this.step_image(dx < 0.0);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(picture)
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
                    .child("←"),
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
        self.detail_select_mode = false;
        self.tts_segments = segments;
        self.tts_index = 0;
        self.tts_active = true;
        self.tts_playing = true;
        self.tts_segment_started = std::time::Instant::now();
        self.speaking = true;
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
            .border_b_1()
            .border_color(border)
            .flex()
            .items_center()
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
                    .bg(if vote_active { accent } else { surface })
                    .border_1()
                    .border_color(if vote_active { accent } else { border })
                    .text_sm()
                    .text_color(if vote_active { rgb(0x0b0e11) } else { fg })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vote_up();
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::ThumbsUp).size(px(14.)))
                    .child(SharedString::from(format!("{} 赞同", detail.vote_up_count))),
            )
            .child(
                div()
                    .id("vote-down")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(if down_active { danger } else { surface })
                    .border_1()
                    .border_color(if down_active { danger } else { border })
                    .text_sm()
                    .text_color(if down_active { rgb(0xffffff) } else { muted })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vote_down();
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::ThumbsDown).size(px(14.)))
                    .child(Icon::new(IconName::ThumbsDown).size(px(12.)))
                    .child("反对"),
            )
            .child(
                div()
                    .id("follow-author")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(if self.detail_following { muted } else { accent })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_follow();
                        cx.notify();
                    }))
                    .child(if self.detail_following {
                        "已关注"
                    } else {
                        "+ 关注"
                    }),
            )
            .child(
                div()
                    .id("open-comments")
                    .px_3()
                    .py_1()
                    .rounded_full()
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
                    .child(SharedString::from(format!("{} 评论", detail.comment_count))),
            )
            .children(detail.question_id.clone().map(|question_id| {
                let title = detail.title.clone();
                div()
                    .id("open-question")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(fg)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_question(question_id.clone(), title.clone());
                        cx.notify();
                    }))
                    .child("全部回答")
            }))
            .child(
                div()
                    .id("block-author")
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
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
                    }))
                    .child("屏蔽"),
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

fn notification_card(
    notification: &zhihu_types::NotificationEntry,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
) -> Div {
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
    header =
        header
            .child(avatar)
            .child(actor)
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child(SharedString::from(
                format_comment_time(notification.created),
            )));

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
    allow_replies: bool,
    states: &HashMap<String, (i8, bool)>,
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
                .bg(if reaction == 1 { accent } else { surface })
                .text_color(if reaction == 1 { rgb(0x0b0e11) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_up
                        .update(cx, |this, cx| this.set_comment_reaction(&id, 1, cx))
                        .ok();
                })
                .child(Icon::new(IconName::ThumbsUp).size(px(12.)))
                .child("赞同"),
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
                .bg(if reaction == -1 {
                    rgb(0xf6465d)
                } else {
                    surface
                })
                .text_color(if reaction == -1 { rgb(0xffffff) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_down
                        .update(cx, |this, cx| this.set_comment_reaction(&id, -1, cx))
                        .ok();
                })
                .child(Icon::new(IconName::ThumbsDown).size(px(12.)))
                .child("反对"),
        );
        let weak_like = weak.clone();
        let id = comment.id.clone();
        footer = footer.child(
            div()
                .id(SharedString::from(format!("comment-like-{}", comment.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(if liked { accent } else { surface })
                .text_color(if liked { rgb(0x0b0e11) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_like
                        .update(cx, |this, cx| this.toggle_comment_like(&id, liked, cx))
                        .ok();
                })
                .child(Icon::new(IconName::Heart).size(px(12.)))
                .child(SharedString::from(format!("喜欢 {like_count}"))),
        );
    } else {
        footer = footer
            .child(Icon::new(IconName::ThumbsUp).size(px(12.)))
            .child(SharedString::from(format!("{like_count} 赞")));
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
    if allow_replies {
        if let Some(weak) = weak {
            let id = comment.id.clone();
            let reply_count = comment.child_count.max(children.len() as u64);
            let label = if expanded {
                "收起回复".to_string()
            } else if reply_count > 0 {
                format!("{reply_count} 条回复")
            } else {
                "查看回复".to_string()
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
                .line_height(px(22.))
                .text_color(fg)
                .child(SharedString::from(plain)),
        )
        .child(footer);

    if expanded {
        let mut column = div()
            .w_full()
            .mt_1()
            .pl_3()
            .border_l_1()
            .border_color(border)
            .flex()
            .flex_col()
            .gap_2();
        let mut previous_author = String::new();
        let mut indent = 0usize;
        for (index, child) in children.iter().enumerate() {
            let target = child.reply_to.as_deref();
            indent = if target == Some(previous_author.as_str()) {
                (indent + 1).min(4)
            } else {
                0
            };
            previous_author = child.author.name.clone();
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
            ));
        }
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
                .bg(if reaction == 1 { accent } else { surface })
                .text_color(if reaction == 1 { rgb(0x0b0e11) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_up
                        .update(cx, |this, cx| this.set_comment_reaction(&id, 1, cx))
                        .ok();
                })
                .child(Icon::new(IconName::ThumbsUp).size(px(12.)))
                .child("赞同"),
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
                .bg(if reaction == -1 {
                    rgb(0xf6465d)
                } else {
                    surface
                })
                .text_color(if reaction == -1 { rgb(0xffffff) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_down
                        .update(cx, |this, cx| this.set_comment_reaction(&id, -1, cx))
                        .ok();
                })
                .child(Icon::new(IconName::ThumbsDown).size(px(12.)))
                .child("反对"),
        );
        let weak_like = weak.clone();
        let id = child.id.clone();
        actions = actions.child(
            div()
                .id(SharedString::from(format!("child-like-{}", child.id)))
                .px_1()
                .py_0p5()
                .rounded_sm()
                .flex()
                .items_center()
                .gap_1()
                .bg(if liked { accent } else { surface })
                .text_color(if liked { rgb(0x0b0e11) } else { muted })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    weak_like
                        .update(cx, |this, cx| this.toggle_comment_like(&id, liked, cx))
                        .ok();
                })
                .child(Icon::new(IconName::Heart).size(px(12.)))
                .child(SharedString::from(format!("喜欢 {like_count}"))),
        );
    } else {
        actions = actions
            .child(Icon::new(IconName::ThumbsUp).size(px(12.)))
            .child(SharedString::from(format!("{like_count} 赞")));
    }
    div()
        .w_full()
        .ml(px(indent as f32 * 14.0))
        .pl_2()
        .border_l_1()
        .border_color(muted)
        .flex()
        .flex_col()
        .gap_1()
        .child(header)
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(SharedString::from(plain)),
        )
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
            children: vec![zhihu_types::Comment {
                id: "demo-c1-1".to_string(),
                content: "<p>同感，尤其是输入法和生命周期那两段。</p>".to_string(),
                author: author("读者乙"),
                reply_to: Some("读者甲".to_string()),
                like_count: 2,
                child_count: 0,
                created_time: now - 1_200,
                is_author: false,
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
            children: Vec::new(),
            child_next_offset: None,
        },
    ]
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
) -> AnyElement {
    let danger = rgb(0xf6465d);
    let meta = format!("{} 赞 · {} 评论", item.vote_up_count, item.comment_count);
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
    let content = div()
        .id(SharedString::from(format!("feed-body-{}", item.id)))
        .aria_label(item.title.clone())
        .w_full()
        .px_4()
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
        })
        .child(
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
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg)
                        .line_clamp(2)
                        .child(SharedString::from(item.title.clone())),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .line_clamp(3)
                        .child(SharedString::from(item.excerpt.clone())),
                ),
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
        let comments = Route::Comments {
            kind: ContentKind::Answer,
            id: "123".to_string(),
            title: "标题".to_string(),
        };
        assert_eq!(route_content_target(&comments), None);
        assert_eq!(route_content_target(&Route::Notifications), None);
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
            },
            reply_to: None,
            like_count: likes,
            child_count: 0,
            created_time: created,
            is_author: false,
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
