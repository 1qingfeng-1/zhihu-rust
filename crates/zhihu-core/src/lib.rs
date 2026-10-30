//! Headless application core: services, routing and view models.
//!
//! This crate deliberately does not depend on GPUI so it can be exercised with
//! plain `cargo test`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tokio::sync::Mutex as AsyncMutex;
use zhihu_api::{PhoneLoginSession, ZhihuClient};
pub use zhihu_api::{SearchOptions, SearchSort, SearchVertical};
use zhihu_recommend::{
    item_key, normalize_vector, select_embedding_provider, CancelToken, Candidate, CollectReport,
    CollectSource, CollectorPolicy, EmbeddingProvider, FeedbackRecord, HiddenRules, RecommendMode,
    RecommendPoolStats, RecommendPreset, VectorProfile, FEEDBACK_HALF_LIFE_DAYS,
};
use zhihu_store::{Store, DB_FILE_NAME};
use zhihu_types::{
    AccountProfile, Comment, ContentDetail, ContentKind, DailyStory, FeedItem, HistoryEntry,
    HotSearchItem, NotificationEntry,
};

/// At most this many missing vectors are generated per ranking pass so the UI
/// thread never blocks on a full pool embed. The rest are filled on later
/// passes.
const MAX_EMBED_PER_CALL: usize = 96;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecommendationMode {
    #[default]
    Web,
    Android,
    Local,
    Hybrid,
}

#[derive(Clone, Debug)]
pub struct AppSettings {
    pub dark: bool,
    pub font_size: f32,
    pub line_height: f32,
    pub tab: usize,
    pub tts_rate: f32,
    pub tts_pitch: f32,
    pub tts_volume: f32,
    pub tts_voice: String,
    pub recommend_mode: RecommendMode,
    /// Fixed ranking preset: `balanced`, `focused`, `explore` or `fresh`.
    pub recommend_preset: RecommendPreset,
    /// Typography preset key: `standard`, `relaxed`, `compact` or `custom`.
    pub typography: String,
    pub reading_indent: bool,
    pub pos_colors: bool,
    pub hide_mcn: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark: true,
            font_size: 16.0,
            line_height: 28.0,
            tab: 0,
            tts_rate: 1.0,
            tts_pitch: 1.0,
            tts_volume: 1.0,
            tts_voice: String::new(),
            recommend_mode: RecommendMode::Off,
            recommend_preset: RecommendPreset::Balanced,
            typography: "standard".to_string(),
            reading_indent: true,
            pos_colors: true,
            hide_mcn: false,
        }
    }
}

impl AppSettings {
    pub fn load(store: &Store) -> Self {
        let mut settings = Self::default();
        if let Ok(Some(value)) = store.get_setting("theme") {
            settings.dark = value != "light";
        }
        if let Ok(Some(value)) = store.get_setting("font_size") {
            settings.font_size = value.parse().unwrap_or(settings.font_size);
        }
        if let Ok(Some(value)) = store.get_setting("line_height") {
            settings.line_height = value.parse().unwrap_or(settings.line_height);
        }
        if let Ok(Some(value)) = store.get_setting("tab") {
            settings.tab = value.parse().unwrap_or(0).min(4);
        }
        if let Ok(Some(value)) = store.get_setting("recommend_mode") {
            settings.recommend_mode = RecommendMode::parse(&value);
        }
        if let Ok(Some(value)) = store.get_setting("recommend_preset") {
            settings.recommend_preset = RecommendPreset::parse(&value);
        }
        if let Ok(Some(value)) = store.get_setting("tts_rate") {
            settings.tts_rate = value.parse::<f32>().unwrap_or(1.0).clamp(0.5, 2.0);
        }
        if let Ok(Some(value)) = store.get_setting("tts_pitch") {
            settings.tts_pitch = value.parse::<f32>().unwrap_or(1.0).clamp(0.5, 2.0);
        }
        if let Ok(Some(value)) = store.get_setting("tts_volume") {
            settings.tts_volume = value.parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0);
        }
        if let Ok(Some(value)) = store.get_setting("tts_voice") {
            settings.tts_voice = value;
        }
        if let Ok(Some(value)) = store.get_setting("typography") {
            settings.typography = value;
        } else if (settings.font_size - 16.0).abs() > 0.1
            || (settings.line_height - 28.0).abs() > 0.1
        {
            // Existing installs that tuned size/line-height before presets
            // existed should not be silently reset to "standard".
            settings.typography = "custom".to_string();
        }
        if let Ok(Some(value)) = store.get_setting("reading_indent") {
            settings.reading_indent = value != "0";
        }
        if let Ok(Some(value)) = store.get_setting("pos_colors") {
            settings.pos_colors = value != "0";
        }
        if let Ok(Some(value)) = store.get_setting("hide_mcn") {
            settings.hide_mcn = value != "0";
        }
        settings
    }

    pub fn save(&self, store: &Store) {
        let _ = store.set_setting("theme", if self.dark { "dark" } else { "light" });
        let _ = store.set_setting("font_size", &self.font_size.to_string());
        let _ = store.set_setting("line_height", &self.line_height.to_string());
        let _ = store.set_setting("tab", &self.tab.to_string());
        let _ = store.set_setting("recommend_mode", self.recommend_mode.as_str());
        let _ = store.set_setting("recommend_preset", self.recommend_preset.as_str());
        let _ = store.set_setting("tts_rate", &self.tts_rate.to_string());
        let _ = store.set_setting("tts_pitch", &self.tts_pitch.to_string());
        let _ = store.set_setting("tts_volume", &self.tts_volume.to_string());
        let _ = store.set_setting("tts_voice", &self.tts_voice);
        let _ = store.set_setting("typography", &self.typography);
        let _ = store.set_setting(
            "reading_indent",
            if self.reading_indent { "1" } else { "0" },
        );
        let _ = store.set_setting("pos_colors", if self.pos_colors { "1" } else { "0" });
        let _ = store.set_setting("hide_mcn", if self.hide_mcn { "1" } else { "0" });
    }
}

#[derive(Debug)]
pub enum UiEvent {
    HotSearch(Vec<HotSearchItem>),
    Daily(Vec<DailyStory>),
    DailyMore(Vec<DailyStory>),
    DailyMoreError(String),
    DailyDetail(zhihu_types::DailyDetail),
    DailyDetailError(String),
    Feed {
        items: Vec<FeedItem>,
        is_end: bool,
    },
    FeedMore {
        items: Vec<FeedItem>,
        is_end: bool,
    },
    FeedMoreError(String),
    FollowFeed {
        kind: zhihu_types::FollowFeedKind,
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
        next: Option<String>,
    },
    FollowFeedError {
        kind: zhihu_types::FollowFeedKind,
        message: String,
    },
    FeedOffline {
        items: Vec<FeedItem>,
    },
    /// Cached recommendation snapshot painted before the network reply
    /// (stale-while-revalidate).
    FeedCached(Vec<FeedItem>),
    Detail(ContentDetail),
    DetailOffline(ContentDetail),
    DetailError(String),
    Question(zhihu_types::QuestionDetail),
    QuestionError(String),
    QuestionAnswers {
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    QuestionAnswersError(String),
    QuestionFound {
        id: String,
        title: String,
    },
    QuestionNotFound(String),
    Topic(zhihu_types::TopicDetail),
    TopicError(String),
    TopicFeed {
        topic_id: String,
        generation: u64,
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
        next: Option<String>,
    },
    TopicFeedError {
        topic_id: String,
        generation: u64,
        message: String,
    },
    TopicFollowed(bool),
    TopicFollowError(String),
    Member(zhihu_types::MemberProfile),
    MemberError(String),
    MemberAnswers {
        kind: ContentKind,
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    MemberAnswersError(String),
    MemberActivities {
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    MemberActivitiesError(String),
    MemberCollections {
        items: Vec<zhihu_types::Collection>,
        append: bool,
        is_end: bool,
    },
    MemberCollectionsError(String),
    MemberColumns {
        items: Vec<zhihu_types::MemberColumn>,
        append: bool,
        is_end: bool,
    },
    MemberColumnsError(String),
    MemberPeople {
        following: bool,
        items: Vec<zhihu_types::MemberProfile>,
        append: bool,
        is_end: bool,
    },
    MemberPeopleError(String),
    MemberFollowed(bool),
    MemberBlocked(bool),
    MemberBlockError(String),
    Collections {
        items: Vec<zhihu_types::Collection>,
        append: bool,
        is_end: bool,
    },
    CollectionsError(String),
    CollectionItems {
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    CollectionItemsError(String),
    CollectionDetail(zhihu_types::Collection),
    ContentCollections(Vec<zhihu_types::Collection>),
    ContentCollectionsError(String),
    FavoriteUpdated {
        collection_id: String,
        favorite: bool,
    },
    FavoriteError(String),
    CollectionCreated(zhihu_types::Collection),
    CollectionCreateError(String),
    CommentLikeUpdated {
        comment_id: String,
        liked: bool,
    },
    CommentLikeError(String),
    CommentSubmitted {
        target_kind: ContentKind,
        target_id: String,
        root_id: Option<String>,
        comment: Comment,
    },
    CommentSubmitError(String),
    CommentAnchor {
        root: Box<Comment>,
        target: Box<Comment>,
        is_child: bool,
    },
    CommentAnchorError(String),
    Account(AccountProfile),
    AccountError(String),
    VoteUpdated(u64),
    FollowUpdated(bool),
    ActionError(String),
    SearchSuggestions(Vec<String>),
    SearchError(String),
    SearchResults {
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    SearchResultsError(String),
    CommentsPage {
        kind: ContentKind,
        target_id: String,
        items: Vec<Comment>,
        append: bool,
        is_end: bool,
    },
    CommentsError {
        kind: ContentKind,
        target_id: String,
        message: String,
    },
    ChildComments {
        parent_id: String,
        items: Vec<Comment>,
        append: bool,
        is_end: bool,
    },
    ChildCommentsError {
        parent_id: String,
        message: String,
    },
    NotificationsRead,
    NotificationsReadError(String),
    Notifications {
        items: Vec<NotificationEntry>,
        timeline_len: usize,
        append: bool,
        is_end: bool,
        total_unread: u64,
    },
    NotificationsError(String),
    NotificationUnread(u64),
    QrCode {
        link: String,
        token: String,
    },
    QrStatus(String),
    QrSuccess,
    QrError(String),
    QrRisk {
        message: String,
        redirect: Option<String>,
        /// Cookies from the QR session, so the verification WebView can share
        /// the same identity and hand them back after verification.
        cookies: Vec<(String, String)>,
    },
    PhoneCodeSent,
    PhoneCaptcha {
        image: Option<PathBuf>,
    },
    PhoneCaptchaVerified(bool),
    PhoneLoginSuccess,
    PhoneLoginError(String),
    ImageReady {
        url: String,
        path: PathBuf,
    },
    ImageFailed {
        url: String,
        error: String,
    },
    /// Manual local-recommendation collection finished (M8-1).
    RecommendCollected(CollectReport),
    /// Background embedding pass finished (M8-3).
    RecommendEmbeddings(EmbeddingReady),
    /// A fresh cold-start poster (image already downloaded) is ready.
    SplashReady {
        image: PathBuf,
        quote: String,
        source: String,
    },
    Error(String),
}

/// One cold-start poster plus the quote shown over it.
#[derive(Clone, Debug)]
pub struct SplashCard {
    pub image_path: PathBuf,
    pub quote: String,
    pub source: String,
}

/// Result of one background embedding pass.
#[derive(Clone, Debug)]
pub struct EmbeddingReady {
    pub profile: Option<VectorProfile>,
    pub error: Option<String>,
    pub elapsed_ms: u128,
}

/// Manual local-recommendation collection request (M8-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectRequest {
    pub sources: Vec<CollectSource>,
    pub policy: CollectorPolicy,
}

impl Default for CollectRequest {
    fn default() -> Self {
        Self {
            sources: CollectSource::manual_defaults(),
            policy: CollectorPolicy::default(),
        }
    }
}

/// Network runtime shared by the UI.
///
/// Network requests run on a Tokio runtime owned by this struct; results are
/// delivered to the GPUI thread through an `async_channel`.
pub struct AppServices {
    client: Arc<ZhihuClient>,
    store: Arc<Store>,
    image_dir: PathBuf,
    splash_dir: PathBuf,
    image_client: reqwest::Client,
    pending_images: Arc<Mutex<HashSet<String>>>,
    phone_login: Arc<AsyncMutex<Option<PhoneLoginSession>>>,
    sender: async_channel::Sender<UiEvent>,
    receiver: async_channel::Receiver<UiEvent>,
    runtime: tokio::runtime::Runtime,
    /// ONNX when the bundled model loads, deterministic hashing otherwise.
    embedding: Arc<dyn EmbeddingProvider>,
    embedding_fallback: bool,
    embedding_busy: Arc<AtomicBool>,
    hot_fetching: Arc<AtomicBool>,
    daily_fetching: Arc<AtomicBool>,
    feed_fetching: Arc<AtomicBool>,
    prefetch_started: AtomicBool,
    /// Serialises splash-index reads/writes between the UI thread and the
    /// background refill, without holding it across network calls.
    splash_lock: Arc<Mutex<()>>,
    splash_fetching: Arc<AtomicBool>,
}

impl AppServices {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        let store = Arc::new(Store::open(data_dir.join(DB_FILE_NAME))?);
        store.save_account(0, "", None, true)?;

        let client = Arc::new(ZhihuClient::new()?);
        let cookies = store.load_cookies(0)?;
        if !cookies.is_empty() {
            client.set_cookies(cookies);
        }

        let image_dir = data_dir.join("images");
        std::fs::create_dir_all(&image_dir)?;
        prune_image_cache(&image_dir, 300);
        let splash_dir = data_dir.join("splash");
        std::fs::create_dir_all(&splash_dir)?;
        let image_client = reqwest::Client::builder()
            .user_agent(zhihu_api::DEFAULT_USER_AGENT)
            .build()?;

        let (sender, receiver) = async_channel::unbounded();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (embedding, embedding_fallback) = select_embedding_provider();
        Ok(Self {
            client,
            store,
            image_dir,
            splash_dir,
            image_client,
            pending_images: Arc::new(Mutex::new(HashSet::new())),
            phone_login: Arc::new(AsyncMutex::new(None)),
            sender,
            receiver,
            runtime,
            embedding: Arc::from(embedding),
            embedding_fallback,
            embedding_busy: Arc::new(AtomicBool::new(false)),
            hot_fetching: Arc::new(AtomicBool::new(false)),
            daily_fetching: Arc::new(AtomicBool::new(false)),
            feed_fetching: Arc::new(AtomicBool::new(false)),
            prefetch_started: AtomicBool::new(false),
            splash_lock: Arc::new(Mutex::new(())),
            splash_fetching: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn client(&self) -> Arc<ZhihuClient> {
        self.client.clone()
    }

    pub fn receiver(&self) -> async_channel::Receiver<UiEvent> {
        self.receiver.clone()
    }

    /// The poster the next cold start should paint, if the pool has one.
    ///
    /// The pool rotates: each cold start consumes the oldest poster that has
    /// not been shown yet and marks it, and [`Self::after_splash_shown`] drops
    /// it once the splash is gone. Marking here (rather than at cleanup time)
    /// keeps a crash during the splash from showing the same photo twice.
    pub fn cached_splash(&self) -> Option<SplashCard> {
        let _guard = self.splash_lock.lock().ok()?;
        let mut index = load_splash_index(&self.splash_dir);
        if index.cards.is_empty() {
            return None;
        }
        // If everything was already shown (the app died before cleanup), reuse
        // the first card instead of falling back to the built-in quotes.
        let position = index.cards.iter().position(|card| !card.shown).unwrap_or(0);
        index.cards[position].shown = true;
        let card = index.cards[position].clone();
        save_splash_index(&self.splash_dir, &index);
        Some(SplashCard {
            image_path: self.splash_dir.join(&card.image),
            quote: card.quote,
            source: card.source,
        })
    }

    /// Top the rotation pool up to [`SPLASH_MAX_CARDS`] posters in the
    /// background. A full pool costs no network at all.
    pub fn refresh_splash(&self) {
        self.spawn_splash_refill(false);
    }

    /// Called once the splash has faded: delete the poster that was just shown
    /// and refill the pool, so it happens while the user is reading the feed.
    pub fn after_splash_shown(&self) {
        self.spawn_splash_refill(true);
    }

    fn spawn_splash_refill(&self, clean_shown: bool) {
        if self.splash_fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        let client = self.image_client.clone();
        let sender = self.sender.clone();
        let dir = self.splash_dir.clone();
        let lock = self.splash_lock.clone();
        let fetching = self.splash_fetching.clone();
        self.runtime.spawn(async move {
            if clean_shown {
                remove_shown_splash(&dir, &lock);
            }
            if let Some(card) = refill_splash(&client, &dir, &lock).await {
                let _ = sender
                    .send(UiEvent::SplashReady {
                        image: card.image_path,
                        quote: card.quote,
                        source: card.source,
                    })
                    .await;
            }
            fetching.store(false, Ordering::SeqCst);
        });
    }

    pub fn refresh_hot_search(&self) {
        if self.hot_fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        // Paint the last snapshot immediately, then refresh in the background
        // (stale-while-revalidate).
        if let Some(items) = cached_hot(&self.store) {
            let _ = self.sender.try_send(UiEvent::HotSearch(items));
        }
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        let fetching = self.hot_fetching.clone();
        self.runtime.spawn(async move {
            let event = match client.hot_search().await {
                Ok(items) => {
                    let patterns = store.block_keywords().unwrap_or_default();
                    let items: Vec<_> = items
                        .into_iter()
                        .filter(|item| !is_blocked_text(&item.query, &patterns))
                        .collect();
                    if let Ok(payload) = serde_json::to_string(&items) {
                        let _ = store.cache_feed("hot", &payload, unix_now());
                    }
                    UiEvent::HotSearch(items)
                }
                Err(error) => UiEvent::Error(format!("热榜加载失败：{error}")),
            };
            fetching.store(false, Ordering::SeqCst);
            let _ = sender.send(event).await;
        });
    }

    pub fn refresh_daily(&self) {
        if self.daily_fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(items) = cached_daily(&self.store) {
            let _ = self.sender.try_send(UiEvent::Daily(items));
        }
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        let fetching = self.daily_fetching.clone();
        self.runtime.spawn(async move {
            let event = match client.daily_latest().await {
                Ok(items) => {
                    let patterns = store.block_keywords().unwrap_or_default();
                    let items: Vec<_> = items
                        .into_iter()
                        .filter(|item| !is_blocked_text(&item.title, &patterns))
                        .collect();
                    if let Ok(payload) = serde_json::to_string(&items) {
                        let _ = store.cache_feed("daily", &payload, unix_now());
                    }
                    UiEvent::Daily(items)
                }
                Err(error) => UiEvent::Error(format!("日报加载失败：{error}")),
            };
            fetching.store(false, Ordering::SeqCst);
            let _ = sender.send(event).await;
        });
    }

    pub fn load_more_daily(&self, date: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.daily_before(&date).await {
                Ok((_, items)) => {
                    let patterns = store.block_keywords().unwrap_or_default();
                    UiEvent::DailyMore(
                        items
                            .into_iter()
                            .filter(|item| !is_blocked_text(&item.title, &patterns))
                            .collect(),
                    )
                }
                Err(error) => UiEvent::DailyMoreError(format!("更早日报加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_daily(&self, id: u64) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.daily_story(id).await {
                Ok(detail) => UiEvent::DailyDetail(detail),
                Err(error) => UiEvent::DailyDetailError(format!("日报加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    /// One-shot warm-up after the first frame. Hot search and the daily feed
    /// are public; the recommendation feed is only prefetched with a session
    /// because its endpoint is rate sensitive.
    pub fn prefetch_startup(&self) {
        if self.prefetch_started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.refresh_hot_search();
        self.refresh_daily();
        if self.is_logged_in() {
            self.refresh_feed();
        }
    }

    pub fn is_logged_in(&self) -> bool {
        self.client.cookie_value("z_c0").is_some()
    }

    pub fn current_account_name(&self) -> Option<String> {
        self.store
            .current_account()
            .ok()
            .flatten()
            .map(|(_, name)| name)
    }

    /// Start a QR login session on a fresh client (the main jar's synthetic
    /// `d_c0` would make the QR endpoint reject the request).
    pub fn start_qr_login(&self) {
        self.start_qr_login_with_cookies(Vec::new());
    }

    /// Restart QR login with cookies carried over from a verification WebView.
    /// A 403 invalidates the old QR token, so the caller must request a new one.
    pub fn start_qr_login_with_cookies(&self, seed: Vec<(String, String)>) {
        let main = self.client.clone();
        let store = self.store.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            // Zhihu's anti-bot layer occasionally rejects the first anonymous
            // QR request even on a clean network. Retry once with a new
            // cookie jar; if it is still risk-controlled, surface the
            // verification link instead of a bare error.
            let mut login = None;
            let mut session = None;
            let mut last_error = String::new();
            for attempt in 0..2 {
                if attempt > 0 {
                    tokio::time::sleep(Duration::from_millis(1_200)).await;
                }
                let client = match ZhihuClient::new() {
                    Ok(client) => client,
                    Err(error) => {
                        last_error = format!("二维码登录初始化失败：{error}");
                        continue;
                    }
                };
                if !seed.is_empty() {
                    client.set_cookies(seed.clone());
                }
                match client.qr_login_start().await {
                    Ok(started) => {
                        login = Some(client);
                        session = Some(started);
                        break;
                    }
                    Err(error) => {
                        let text = error.to_string();
                        if let Some((message, redirect)) = parse_qr_risk(&text) {
                            if attempt == 0 {
                                last_error = text;
                                continue;
                            }
                            let _ = sender
                                .send(UiEvent::QrRisk {
                                    message,
                                    redirect,
                                    cookies: client.all_cookies(),
                                })
                                .await;
                            return;
                        }
                        last_error = text;
                    }
                }
            }
            let (Some(login), Some(session)) = (login, session) else {
                let _ = sender
                    .send(UiEvent::QrError(format!("二维码获取失败：{last_error}")))
                    .await;
                return;
            };
            let _ = sender
                .send(UiEvent::QrCode {
                    link: session.link.clone(),
                    token: session.token.clone(),
                })
                .await;

            let deadline = Instant::now() + Duration::from_secs(120);
            while Instant::now() < deadline {
                if let Ok(status) = login.qr_login_poll(&session.token).await {
                    if status.risk_control {
                        let message = status
                            .message
                            .unwrap_or_else(|| "知乎要求验证网络环境，请先完成验证".to_string());
                        let _ = sender
                            .send(UiEvent::QrRisk {
                                message,
                                redirect: status.redirect,
                                cookies: login.all_cookies(),
                            })
                            .await;
                        return;
                    }
                    if status.success {
                        let cookies = login.all_cookies();
                        main.set_cookies(cookies.clone());
                        let _ = store.save_cookies(0, &cookies);
                        if let Ok(profile) = main.me_profile().await {
                            let _ = store.save_account(
                                0,
                                &profile.name,
                                profile.url_token.as_deref(),
                                true,
                            );
                            let _ = sender.send(UiEvent::Account(profile)).await;
                        }
                        let _ = sender.send(UiEvent::QrSuccess).await;
                        return;
                    }
                    let upper = status.login_status.to_uppercase();
                    if upper.contains("EXPIRED") || status.qr_status == 2 {
                        let _ = sender
                            .send(UiEvent::QrError("二维码已过期，请重新生成".into()))
                            .await;
                        return;
                    }
                    if status.qr_status == 1 || upper.contains("SCAN") {
                        let _ = sender
                            .send(UiEvent::QrStatus("已扫码，请在知乎 App 上确认".into()))
                            .await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(1_500)).await;
            }
            let _ = sender
                .send(UiEvent::QrError("二维码已过期，请重新生成".into()))
                .await;
        });
    }

    /// Request an SMS code. Keeps the guest session alive for the sign-in step.
    pub fn phone_request_code(&self, phone: String) {
        let session = self.phone_login.clone();
        let sender = self.sender.clone();
        let image_dir = self.image_dir.clone();
        self.runtime.spawn(async move {
            let mut guard = session.lock().await;
            if guard.is_none() {
                match PhoneLoginSession::new() {
                    Ok(login) => *guard = Some(login),
                    Err(error) => {
                        let _ = sender
                            .send(UiEvent::PhoneLoginError(format!(
                                "手机号登录初始化失败：{error}"
                            )))
                            .await;
                        return;
                    }
                }
            }
            let Some(login) = guard.as_mut() else {
                return;
            };
            match login.request_digits(&phone).await {
                Ok(zhihu_api::DigitsOutcome::Sent) => {
                    let _ = sender.send(UiEvent::PhoneCodeSent).await;
                }
                Ok(zhihu_api::DigitsOutcome::CaptchaRequired { image_base64 }) => {
                    let path = image_base64.and_then(|data| {
                        let bytes = BASE64.decode(data).ok()?;
                        if let Ok(entries) = std::fs::read_dir(&image_dir) {
                            for entry in entries.flatten() {
                                if entry
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with("phone-captcha-")
                                {
                                    let _ = std::fs::remove_file(entry.path());
                                }
                            }
                        }
                        let stamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|duration| duration.as_millis())
                            .unwrap_or_default();
                        let path = image_dir.join(format!("phone-captcha-{stamp}.png"));
                        std::fs::write(&path, bytes).ok()?;
                        Some(path)
                    });
                    let _ = sender.send(UiEvent::PhoneCaptcha { image: path }).await;
                }
                Err(error) => {
                    let _ = sender
                        .send(UiEvent::PhoneLoginError(error.to_string()))
                        .await;
                }
            }
        });
    }

    /// Submit an image captcha, then the caller should request the code again.
    pub fn phone_verify_captcha(&self, input: String) {
        let session = self.phone_login.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let mut guard = session.lock().await;
            let Some(login) = guard.as_mut() else {
                let _ = sender
                    .send(UiEvent::PhoneLoginError("请先获取短信验证码".into()))
                    .await;
                return;
            };
            match login.verify_captcha(&input).await {
                Ok(verified) => {
                    let _ = sender.send(UiEvent::PhoneCaptchaVerified(verified)).await;
                }
                Err(error) => {
                    let _ = sender
                        .send(UiEvent::PhoneLoginError(error.to_string()))
                        .await;
                }
            }
        });
    }

    /// Exchange the SMS code for cookies and persist the account.
    pub fn phone_sign_in(&self, phone: String, code: String) {
        let session = self.phone_login.clone();
        let main = self.client.clone();
        let store = self.store.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let outcome = {
                let mut guard = session.lock().await;
                match guard.as_mut() {
                    Some(login) => login
                        .sign_in(&phone, &code)
                        .await
                        .map(|token| token.cookies)
                        .map_err(|error| error.to_string()),
                    None => Err("请先获取短信验证码".to_string()),
                }
            };
            let cookies = match outcome {
                Ok(cookies) => cookies,
                Err(error) => {
                    let _ = sender.send(UiEvent::PhoneLoginError(error)).await;
                    return;
                }
            };
            main.set_cookies(cookies.clone());
            let cookie_pairs: Vec<(String, String)> = cookies
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            if let Err(error) = store.save_cookies(0, &cookie_pairs) {
                log::warn!("failed to persist phone-login cookies: {error}");
            }
            *session.lock().await = None;
            match main.me_profile().await {
                Ok(profile) => {
                    let _ =
                        store.save_account(0, &profile.name, profile.url_token.as_deref(), true);
                    let _ = sender.send(UiEvent::Account(profile)).await;
                }
                Err(error) => {
                    log::warn!("phone login profile check failed: {error}");
                }
            }
            let _ = sender.send(UiEvent::PhoneLoginSuccess).await;
        });
    }

    /// Validate the imported cookies against `/api/v4/me` and persist the name.
    pub fn validate_login(&self) {
        let client = self.client.clone();
        let store = self.store.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            match client.me_profile().await {
                Ok(profile) => {
                    if let Err(error) =
                        store.save_account(0, &profile.name, profile.url_token.as_deref(), true)
                    {
                        log::warn!("failed to persist account profile: {error}");
                    }
                    let _ = sender.send(UiEvent::Account(profile)).await;
                }
                Err(error) => {
                    let _ = sender
                        .send(UiEvent::AccountError(format!("登录校验失败：{error}")))
                        .await;
                }
            }
        });
    }

    pub fn refresh_feed(&self) {
        if self.feed_fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(items) = cached_feed(&self.store) {
            if !items.is_empty() {
                let _ = self.sender.try_send(UiEvent::FeedCached(items));
            }
        }
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        let fetching = self.feed_fetching.clone();
        self.runtime.spawn(async move {
            let event = match client.feed_recommend_items(1).await {
                Ok((items, is_end)) => {
                    let items = filter_feed_items(items, &store);
                    if let Ok(payload) = serde_json::to_string(&items) {
                        let _ = store.cache_feed("recommend", &payload, unix_now());
                    }
                    UiEvent::Feed { items, is_end }
                }
                Err(error) => match cached_feed(&store) {
                    Some(items) => UiEvent::FeedOffline { items },
                    None => UiEvent::Error(format!("推荐流加载失败：{error}")),
                },
            };
            fetching.store(false, Ordering::SeqCst);
            let _ = sender.send(event).await;
        });
    }

    /// Append the next recommendation page (page numbers start at 1).
    pub fn load_more_feed(&self, page: u32) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.feed_recommend_items(page).await {
                Ok((items, is_end)) => UiEvent::FeedMore {
                    items: filter_feed_items(items, &store),
                    is_end,
                },
                Err(error) => UiEvent::FeedMoreError(format!("加载更多失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_follow_feed(
        &self,
        kind: zhihu_types::FollowFeedKind,
        next: Option<String>,
        append: bool,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.follow_feed(kind, next.as_deref()).await {
                Ok(page) => UiEvent::FollowFeed {
                    kind,
                    items: filter_feed_items(page.items, &store),
                    append,
                    is_end: page.is_end,
                    next: page.next,
                },
                Err(error) => UiEvent::FollowFeedError {
                    kind,
                    message: format!("关注{}加载失败：{error}", kind.label()),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_answer(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.answer_detail(&id).await {
                Ok(detail) => {
                    cache_detail(&store, &detail);
                    record_read(&store, &detail);
                    UiEvent::Detail(detail)
                }
                Err(error) => match cached_detail(&store, ContentKind::Answer, &id) {
                    Some(detail) => UiEvent::DetailOffline(detail),
                    None => UiEvent::DetailError(format!("回答加载失败：{error}")),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_article(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.article_detail(&id).await {
                Ok(detail) => {
                    cache_detail(&store, &detail);
                    record_read(&store, &detail);
                    UiEvent::Detail(detail)
                }
                Err(error) => match cached_detail(&store, ContentKind::Article, &id) {
                    Some(detail) => UiEvent::DetailOffline(detail),
                    None => UiEvent::DetailError(format!("文章加载失败：{error}")),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_pin(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.pin_detail(&id).await {
                Ok(detail) => {
                    cache_detail(&store, &detail);
                    record_read(&store, &detail);
                    UiEvent::Detail(detail)
                }
                Err(error) => match cached_detail(&store, ContentKind::Pin, &id) {
                    Some(detail) => UiEvent::DetailOffline(detail),
                    None => UiEvent::DetailError(format!("想法加载失败：{error}")),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_question(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.question_detail(&id).await {
                Ok(question) => UiEvent::Question(question),
                Err(error) => UiEvent::QuestionError(format!("问题加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_question_answers(&self, id: String, offset: u32, append: bool, order: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.question_answers(&id, offset, &order).await {
                Ok((items, is_end)) => UiEvent::QuestionAnswers {
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::QuestionAnswersError(format!("回答加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    /// Resolve a hot-search query to its first question, falling back to the
    /// search page when no question is found.
    pub fn find_question(&self, query: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.search_questions(&query).await {
                Ok(items) => match items.into_iter().next() {
                    Some(item) => UiEvent::QuestionFound {
                        id: item.id,
                        title: item.title,
                    },
                    None => UiEvent::QuestionNotFound(query),
                },
                Err(_) => UiEvent::QuestionNotFound(query),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_topic(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.topic_detail(&id).await {
                Ok(topic) => UiEvent::Topic(topic),
                Err(error) => UiEvent::TopicError(format!("话题加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_topic_feed(
        &self,
        id: String,
        tab: zhihu_types::TopicFeedTab,
        sort: zhihu_types::TopicSort,
        next: Option<String>,
        append: bool,
        generation: u64,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        let topic_id = id.clone();
        self.runtime.spawn(async move {
            let event = match client.topic_feed(&id, tab, sort, next.as_deref()).await {
                Ok(page) => UiEvent::TopicFeed {
                    topic_id,
                    generation,
                    items: filter_feed_items(page.items, &store),
                    append,
                    is_end: page.is_end,
                    next: page.next,
                },
                Err(error) => UiEvent::TopicFeedError {
                    topic_id,
                    generation,
                    message: format!("话题{}加载失败：{error}", tab.label()),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn follow_topic(&self, id: String, follow: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.follow_topic(&id, follow).await {
                Ok(()) => UiEvent::TopicFollowed(follow),
                Err(error) => UiEvent::TopicFollowError(format!("话题关注失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member(&self, token: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member(&token).await {
                Ok(member) => UiEvent::Member(member),
                Err(error) => UiEvent::MemberError(format!("用户加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_answers(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_answers(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberAnswers {
                    kind: ContentKind::Answer,
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberAnswersError(format!("用户回答加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_articles(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_articles(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberAnswers {
                    kind: ContentKind::Article,
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberAnswersError(format!("用户文章加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_pins(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_pins(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberAnswers {
                    kind: ContentKind::Pin,
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberAnswersError(format!("用户想法加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_questions(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_questions(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberAnswers {
                    kind: ContentKind::Question,
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberAnswersError(format!("用户提问加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_columns(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_columns(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberColumns {
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberColumnsError(format!("用户专栏加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_activities(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_activities(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberActivities {
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberActivitiesError(format!("用户动态加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_collections(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.member_favlists(&token, offset).await {
                Ok((items, is_end)) => UiEvent::MemberCollections {
                    items,
                    append,
                    is_end,
                },
                Err(error) => {
                    UiEvent::MemberCollectionsError(format!("用户收藏夹加载失败：{error}"))
                }
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_member_people(
        &self,
        token: String,
        member_id: String,
        following: bool,
        offset: u32,
        append: bool,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = if following {
                client.member_followees(&token, offset).await
            } else {
                client.member_followers(&token, &member_id, offset).await
            };
            let event = match result {
                Ok((items, is_end)) => UiEvent::MemberPeople {
                    following,
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberPeopleError(format!("用户列表加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn set_member_follow(&self, token: String, follow: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.follow_member(&token, follow).await {
                Ok(_) => UiEvent::MemberFollowed(follow),
                Err(error) => UiEvent::MemberError(format!("关注失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_collections(&self, token: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.collections(&token, offset).await {
                Ok((items, is_end)) => UiEvent::Collections {
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::CollectionsError(format!("收藏夹加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_collection_items(&self, id: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.collection_items(&id, offset).await {
                Ok((items, is_end)) => UiEvent::CollectionItems {
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::CollectionItemsError(format!("收藏内容加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_collection_detail(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.collection_detail(&id).await {
                Ok(collection) => UiEvent::CollectionDetail(collection),
                Err(error) => UiEvent::CollectionItemsError(format!("收藏夹加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_content_collections(&self, kind: ContentKind, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.content_collections(kind, &id).await {
                Ok(items) => UiEvent::ContentCollections(items),
                Err(error) => UiEvent::ContentCollectionsError(format!("收藏夹加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn create_collection(&self, title: String, description: String, is_public: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client
                .create_collection(&title, &description, is_public)
                .await
            {
                Ok(collection) => UiEvent::CollectionCreated(collection),
                Err(error) => UiEvent::CollectionCreateError(format!("新建收藏夹失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn set_favorite(
        &self,
        kind: ContentKind,
        id: String,
        collection_id: String,
        favorite: bool,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client
                .set_favorite(kind, &id, &collection_id, favorite)
                .await
            {
                Ok(()) => UiEvent::FavoriteUpdated {
                    collection_id,
                    favorite,
                },
                Err(error) => UiEvent::FavoriteError(format!("收藏操作失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn submit_comment(
        &self,
        kind: ContentKind,
        id: String,
        text: String,
        reply_to: Option<String>,
        root_id: Option<String>,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client
                .submit_comment(kind, &id, &text, reply_to.as_deref())
                .await
            {
                Ok(comment) => UiEvent::CommentSubmitted {
                    target_kind: kind,
                    target_id: id,
                    root_id,
                    comment,
                },
                Err(error) => UiEvent::CommentSubmitError(format!("评论发送失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_comment_anchor(&self, comment_id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.resolve_comment_anchor(&comment_id).await {
                Ok((root, target, is_child)) => UiEvent::CommentAnchor {
                    root: Box::new(root),
                    target: Box::new(target),
                    is_child,
                },
                Err(error) => UiEvent::CommentAnchorError(format!("评论定位失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn set_member_block(&self, token: String, block: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.block_member(&token, block).await {
                Ok(()) => UiEvent::MemberBlocked(block),
                Err(error) => UiEvent::MemberBlockError(format!("拉黑操作失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn like_comment(&self, id: String, liked: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.like_comment(&id, liked).await {
                Ok(_) => UiEvent::CommentLikeUpdated {
                    comment_id: id,
                    liked,
                },
                Err(error) => UiEvent::CommentLikeError(format!("评论点赞失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn vote(&self, kind: ContentKind, id: String, state: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = match kind {
                ContentKind::Answer => client.vote_answer(&id, &state).await,
                ContentKind::Article => client.vote_article(&id, &state).await,
                ContentKind::Pin => client.vote_pin(&id, state == "up").await,
                _ => return,
            };
            let event = match result {
                Ok(count) => UiEvent::VoteUpdated(count),
                Err(error) => UiEvent::ActionError(format!("操作失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    /// Ensure an image is cached on disk. Returns the cached path when ready,
    /// otherwise spawns a download and returns None (UI shows a placeholder).
    pub fn ensure_image(&self, url: &str) -> Option<PathBuf> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return None;
        }
        let path = self.image_path(url);
        if path.exists() {
            return Some(path);
        }
        {
            let mut pending = self.pending_images.lock().expect("image pending mutex");
            if !pending.insert(url.to_string()) {
                return None;
            }
        }

        let client = self.image_client.clone();
        let sender = self.sender.clone();
        let pending = Arc::clone(&self.pending_images);
        let path_for_task = path.clone();
        let url_for_task = url.to_string();
        self.runtime.spawn(async move {
            let result = async {
                let response = client.get(&url_for_task).send().await?;
                if !response.status().is_success() {
                    anyhow::bail!("status {}", response.status());
                }
                let bytes = response.bytes().await?;
                if let Some(parent) = path_for_task.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let tmp = path_for_task.with_extension("part");
                tokio::fs::write(&tmp, &bytes).await?;
                tokio::fs::rename(&tmp, &path_for_task).await?;
                Ok::<(), anyhow::Error>(())
            }
            .await;
            pending
                .lock()
                .expect("image pending mutex")
                .remove(&url_for_task);
            let event = match result {
                Ok(()) => UiEvent::ImageReady {
                    url: url_for_task,
                    path: path_for_task,
                },
                Err(error) => UiEvent::ImageFailed {
                    url: url_for_task,
                    error: error.to_string(),
                },
            };
            let _ = sender.send(event).await;
        });
        None
    }

    fn image_path(&self, url: &str) -> PathBuf {
        let mut hasher = md5::Md5::new();
        use md5::Digest as _;
        hasher.update(url.as_bytes());
        let digest = hasher.finalize();
        let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let extension = url
            .split('?')
            .next()
            .and_then(|path| path.rsplit('.').next())
            .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("img");
        self.image_dir.join(format!("{hash}.{extension}"))
    }

    pub fn settings(&self) -> AppSettings {
        AppSettings::load(&self.store)
    }

    pub fn save_settings(&self, settings: &AppSettings) {
        settings.save(&self.store);
    }

    pub fn history(&self, limit: usize) -> Vec<HistoryEntry> {
        self.store.read_history(limit).unwrap_or_default()
    }

    /// Mark a feed item as read once the reader has actually stayed on it.
    pub fn mark_read(&self, kind: ContentKind, id: &str, title: &str) {
        let target_type = match kind {
            ContentKind::Article => "article",
            ContentKind::Pin => "pin",
            ContentKind::Question => "question",
            _ => "answer",
        };
        if let Err(error) = self.store.record_read(target_type, id, title, unix_now()) {
            log::warn!("failed to mark content read: {error}");
        }
    }

    /// Content ids the reader has opened before, for feed read/unread styling.
    pub fn read_ids(&self) -> std::collections::HashSet<String> {
        self.store
            .read_history(2_000)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| format!("{}:{}", entry.kind, entry.id))
            .collect()
    }

    pub fn save_read_progress(&self, kind: ContentKind, id: &str, progress: f32) {
        let target_type = match kind {
            ContentKind::Article => "article",
            ContentKind::Pin => "pin",
            _ => "answer",
        };
        let now = unix_now();
        if let Err(error) = self
            .store
            .update_read_progress(target_type, id, progress, now)
        {
            log::warn!("failed to save reading progress: {error}");
        }
    }

    pub fn read_progress(&self, kind: ContentKind, id: &str) -> f32 {
        let target_type = match kind {
            ContentKind::Article => "article",
            ContentKind::Pin => "pin",
            _ => "answer",
        };
        self.store.read_progress(target_type, id).unwrap_or(0.0)
    }

    pub fn add_search_history(&self, query: &str) {
        if query.trim().is_empty() {
            return;
        }
        if let Err(error) = self.store.add_search_history(query.trim(), unix_now()) {
            log::warn!("failed to save search history: {error}");
        }
    }

    pub fn search_history(&self, limit: usize) -> Vec<String> {
        self.store.search_history(limit).unwrap_or_default()
    }

    pub fn clear_search_history(&self) {
        if let Err(error) = self.store.clear_search_history() {
            log::warn!("failed to clear search history: {error}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_highlight(
        &self,
        target_type: &str,
        target_id: &str,
        title: &str,
        quote: &str,
        note: &str,
        tags: &str,
        color: &str,
        start_offset: i64,
        end_offset: i64,
    ) -> anyhow::Result<i64> {
        Ok(self.store.add_highlight(
            target_type,
            target_id,
            title,
            quote,
            note,
            tags,
            color,
            start_offset,
            end_offset,
            unix_now(),
        )?)
    }

    pub fn highlights(&self, limit: usize) -> Vec<zhihu_types::Highlight> {
        self.store.highlights(limit).unwrap_or_default()
    }

    pub fn delete_highlight(&self, id: i64) {
        if let Err(error) = self.store.delete_highlight(id) {
            log::warn!("failed to delete highlight: {error}");
        }
    }

    pub fn clear_highlights(&self) {
        if let Err(error) = self.store.clear_highlights() {
            log::warn!("failed to clear highlights: {error}");
        }
    }

    pub fn clear_history(&self) {
        if let Err(error) = self.store.clear_history() {
            log::warn!("failed to clear read history: {error}");
        }
    }

    pub fn recommend_feedback(&self, limit: usize) -> Vec<FeedbackRecord> {
        self.store.recommend_feedback(limit).unwrap_or_default()
    }

    pub fn recommend_feedback_count(&self) -> usize {
        self.store.recommend_feedback_count().unwrap_or(0)
    }

    pub fn disliked_item_ids(&self) -> Vec<String> {
        self.store.disliked_item_ids().unwrap_or_default()
    }

    pub fn record_recommend_feedback(
        &self,
        item_id: &str,
        action: &str,
        author_id: &str,
        topics: &[String],
        weight: f32,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        if let Err(error) = self
            .store
            .record_recommend_feedback(item_id, action, author_id, topics, weight, now)
        {
            log::warn!("failed to record recommendation feedback: {error}");
        }
    }

    pub fn clear_recommend_feedback(&self) {
        if let Err(error) = self.store.clear_recommend_feedback() {
            log::warn!("failed to clear recommendation feedback: {error}");
        }
    }

    /// Collection must be turned on explicitly; defaults to off.
    pub fn recommend_collection_enabled(&self) -> bool {
        matches!(
            self.store
                .get_setting("recommend_collect_enabled")
                .ok()
                .flatten()
                .as_deref(),
            Some("1")
        )
    }

    pub fn set_recommend_collection_enabled(&self, enabled: bool) {
        if let Err(error) = self
            .store
            .set_setting("recommend_collect_enabled", if enabled { "1" } else { "0" })
        {
            log::warn!("failed to update recommendation collection setting: {error}");
        }
    }

    pub fn recommend_pool_stats(&self) -> RecommendPoolStats {
        self.store
            .recommend_pool_stats(unix_now())
            .unwrap_or_default()
    }

    /// Non-expired candidates, newest first. Callers rank them locally.
    pub fn recommend_pool(&self, limit: usize) -> Vec<Candidate> {
        self.store
            .recommend_pool(limit, unix_now())
            .unwrap_or_default()
    }

    pub fn embedding_model_id(&self) -> &str {
        self.embedding.model_id()
    }

    /// True when the bundled ONNX model could not load and the statistical
    /// hash provider is in use.
    pub fn embedding_fallback(&self) -> bool {
        self.embedding_fallback
    }

    pub fn recommend_embedding_count(&self) -> usize {
        self.store
            .recommend_embedding_count(self.embedding.model_id())
            .unwrap_or(0)
    }

    /// Embed arbitrary texts with the active provider (no cache write).
    pub fn embed_recommend_texts(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.embedding
            .embed_batch(texts)
            .unwrap_or_else(|_| vec![vec![0.0; self.embedding.dimension()]; texts.len()])
    }

    /// Ensure the pool is embedded and build the weighted interest center from
    /// feedback items. Returns `None` when the pool is empty.
    ///
    /// Cached vectors are reused; changing the provider/model regenerates them
    /// and prunes other model versions.
    pub fn recommend_vector_profile(&self) -> Option<VectorProfile> {
        build_vector_profile(&self.store, self.embedding.as_ref())
    }

    /// Start a background embedding pass. Returns `false` if one is already
    /// running. The profile arrives as [`UiEvent::RecommendEmbeddings`].
    pub fn start_recommend_embedding(&self) -> bool {
        if self.embedding_busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        let store = self.store.clone();
        let embedding = self.embedding.clone();
        let sender = self.sender.clone();
        let busy = self.embedding_busy.clone();
        self.runtime.spawn(async move {
            let started = Instant::now();
            let result = tokio::task::spawn_blocking(move || {
                build_vector_profile(store.as_ref(), embedding.as_ref())
            })
            .await;
            let (profile, error) = match result {
                Ok(profile) => (profile, None),
                Err(error) => (None, Some(format!("嵌入任务失败：{error}"))),
            };
            busy.store(false, Ordering::SeqCst);
            let _ = sender
                .send(UiEvent::RecommendEmbeddings(EmbeddingReady {
                    profile,
                    error,
                    elapsed_ms: started.elapsed().as_millis(),
                }))
                .await;
        });
        true
    }

    /// Remove every collected candidate. Returns how many rows were dropped.
    pub fn clear_recommend_pool(&self) -> usize {
        let removed = self.store.recommend_pool_count(unix_now()).unwrap_or(0);
        if let Err(error) = self.store.clear_recommend_pool() {
            log::warn!("failed to clear recommendation pool: {error}");
            return 0;
        }
        removed
    }

    /// Run one manual collection pass on the network runtime and report the
    /// outcome as [`UiEvent::RecommendCollected`]. The returned token cancels
    /// the run at the next page boundary.
    pub fn start_recommend_collection(&self, request: CollectRequest) -> CancelToken {
        let cancel = CancelToken::new();
        if !self.recommend_collection_enabled() {
            let _ = self
                .sender
                .try_send(UiEvent::RecommendCollected(CollectReport {
                    sources: request.sources.iter().map(CollectSource::slug).collect(),
                    error: Some("本地推荐采集未开启".to_string()),
                    ..Default::default()
                }));
            return cancel;
        }
        let client = self.client.clone();
        let store = self.store.clone();
        let sender = self.sender.clone();
        let token = cancel.clone();
        self.runtime.spawn(async move {
            let report = run_collection(client.as_ref(), store.as_ref(), &request, &token).await;
            let _ = sender.send(UiEvent::RecommendCollected(report)).await;
        });
        cancel
    }

    pub fn blocked_users(&self) -> Vec<(String, String)> {
        self.store.blocked_users().unwrap_or_default()
    }

    pub fn recommend_hidden_users(&self) -> Vec<(String, String)> {
        self.store.recommend_hidden_users().unwrap_or_default()
    }

    pub fn set_recommend_hidden_user(&self, id: String, name: String, hidden: bool) {
        let result = if hidden {
            self.store.add_recommend_hidden_user(&id, &name)
        } else {
            self.store.remove_recommend_hidden_user(&id)
        };
        if let Err(error) = result {
            log::warn!("failed to update recommendation hidden user: {error}");
        }
    }

    pub fn block_user(&self, id: String, name: String) {
        if id.is_empty() && name.is_empty() {
            return;
        }
        if let Err(error) = self.store.add_block_user(&id, &name) {
            log::warn!("failed to block user: {error}");
        }
    }

    pub fn unblock_user(&self, id: String) {
        if let Err(error) = self.store.remove_block_user(&id) {
            log::warn!("failed to unblock user: {error}");
        }
    }

    pub fn blocked_topics(&self) -> Vec<(String, String)> {
        self.store.blocked_topics().unwrap_or_default()
    }

    pub fn block_topic(&self, id: String, name: String) {
        if let Err(error) = self.store.add_block_topic(&id, &name) {
            log::warn!("failed to block topic: {error}");
        }
    }

    pub fn unblock_topic(&self, id: String) {
        if let Err(error) = self.store.remove_block_topic(&id) {
            log::warn!("failed to unblock topic: {error}");
        }
    }

    pub fn block_keywords(&self) -> Vec<(String, bool)> {
        self.store.block_keywords().unwrap_or_default()
    }

    pub fn add_block_keyword(&self, pattern: String, is_regex: bool) {
        if pattern.trim().is_empty() {
            return;
        }
        if let Err(error) = self.store.add_block_keyword(pattern.trim(), is_regex) {
            log::warn!("failed to add block keyword: {error}");
        }
    }

    pub fn remove_block_keyword(&self, pattern: String) {
        if let Err(error) = self.store.remove_block_keyword(&pattern) {
            log::warn!("failed to remove block keyword: {error}");
        }
    }

    pub fn load_notifications(&self, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.notifications(offset).await {
                Ok(page) => UiEvent::Notifications {
                    items: page.items,
                    timeline_len: page.timeline_len,
                    append,
                    is_end: page.is_end,
                    total_unread: page.total_unread,
                },
                Err(error) => UiEvent::NotificationsError(format!("通知加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn refresh_notification_unread(&self) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            if let Ok(count) = client.notifications_unread().await {
                let _ = sender.send(UiEvent::NotificationUnread(count)).await;
            }
        });
    }

    pub fn mark_notifications_read(&self) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.mark_all_notifications_read().await {
                Ok(()) => UiEvent::NotificationsRead,
                Err(error) => UiEvent::NotificationsReadError(format!("同步已读失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_child_comments(&self, parent_id: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.child_comments(&parent_id, offset).await {
                Ok(page) => UiEvent::ChildComments {
                    parent_id,
                    items: page.items,
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::ChildCommentsError {
                    parent_id,
                    message: format!("回复加载失败：{error}"),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_comments(&self, kind: ContentKind, id: String, offset: u32, append: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let target_id = id.clone();
        self.runtime.spawn(async move {
            let event = match client.comments(kind, &id, offset).await {
                Ok(page) => UiEvent::CommentsPage {
                    kind,
                    target_id,
                    items: page.items,
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::CommentsError {
                    kind,
                    target_id,
                    message: format!("评论加载失败：{error}"),
                },
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn search(
        &self,
        query: String,
        offset: u32,
        append: bool,
        options: zhihu_api::SearchOptions,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.search(&query, offset, options).await {
                Ok(page) => UiEvent::SearchResults {
                    items: page.items,
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::SearchResultsError(format!("搜索失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn search_member(
        &self,
        query: String,
        member_hash: String,
        offset: u32,
        append: bool,
        options: zhihu_api::SearchOptions,
    ) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client
                .search_member(&query, &member_hash, offset, options)
                .await
            {
                Ok(page) => UiEvent::SearchResults {
                    items: page.items,
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::SearchResultsError(format!("搜索 TA 的创作失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn search_suggest(&self, query: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.search_suggest(&query).await {
                Ok(items) => UiEvent::SearchSuggestions(items),
                Err(error) => UiEvent::SearchError(format!("搜索失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn follow_author(&self, url_token: String, follow: bool) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let event = match client.follow_member(&url_token, follow).await {
                Ok(_) => UiEvent::FollowUpdated(follow),
                Err(error) => UiEvent::ActionError(format!("关注失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn load_detail(&self, kind: ContentKind, id: String) {
        match kind {
            ContentKind::Answer => self.load_answer(id),
            ContentKind::Article => self.load_article(id),
            ContentKind::Pin => self.load_pin(id),
            _ => {
                let sender = self.sender.clone();
                let message = format!("暂不支持加载 {:?} 类型", kind);
                self.runtime.spawn(async move {
                    let _ = sender.send(UiEvent::DetailError(message)).await;
                });
            }
        }
    }

    /// Import a browser `Cookie` header (`a=1; b=2`) and persist it.
    pub fn import_cookie_header(&self, raw: &str) -> usize {
        let pairs: Vec<(String, String)> = raw
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (!name.is_empty()).then(|| (name.to_string(), value.to_string()))
            })
            .collect();
        let count = pairs.len();
        self.client.set_cookies(pairs.clone());
        if let Err(error) = self.store.save_cookies(0, &pairs) {
            log::warn!("failed to persist cookies: {error}");
        }
        count
    }

    pub fn cookie_header(&self) -> Option<String> {
        self.client.cookie_header()
    }

    pub fn store(&self) -> Arc<Store> {
        self.store.clone()
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    pub signed_in: bool,
    pub mode: RecommendationMode,
    pub hot_search: Vec<HotSearchItem>,
    pub daily: Vec<DailyStory>,
}

impl AppState {
    pub fn feed_placeholder(&self) -> Vec<FeedItem> {
        zhihu_types::m0_sample_feed()
    }
}

/// Manual collection loop (M8-1).
///
/// Stops at the first request error (risk control, auth loss, network), honors
/// cancellation between pages and never exceeds the sanitized policy. Pages are
/// written as they arrive so a cancelled run keeps what it already collected.
async fn run_collection(
    client: &ZhihuClient,
    store: &Store,
    request: &CollectRequest,
    cancel: &CancelToken,
) -> CollectReport {
    let policy = request.policy.sanitized();
    let mut report = CollectReport {
        sources: request.sources.iter().map(CollectSource::slug).collect(),
        ..Default::default()
    };
    let rules = load_hidden_rules(store);
    let patterns = store.block_keywords().unwrap_or_default();
    let now = unix_now();

    'sources: for source in &request.sources {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }

        if *source == CollectSource::Daily {
            report.pages += 1;
            let stories = match client.daily_latest().await {
                Ok(stories) => stories,
                Err(error) => {
                    report.error = Some(format!("{}采集失败：{error}", source.label()));
                    break 'sources;
                }
            };
            report.fetched += stories.len();
            let candidates: Vec<Candidate> = stories
                .iter()
                .filter_map(|story| {
                    keep_candidate(
                        Candidate::from_daily_story(story, now, policy.retention_days),
                        &rules,
                        &patterns,
                        &mut report,
                    )
                })
                .collect();
            match store.upsert_recommend_candidates(&candidates) {
                Ok((inserted, updated)) => {
                    report.inserted += inserted;
                    report.updated += updated;
                }
                Err(error) => {
                    report.error = Some(format!("候选池写入失败：{error}"));
                    break 'sources;
                }
            }
            continue;
        }

        let mut cursor: Option<String> = None;
        for page in 0..policy.max_pages {
            if page > 0 {
                tokio::time::sleep(policy.throttle).await;
            }
            if cancel.is_cancelled() {
                report.cancelled = true;
                break 'sources;
            }
            let (items, is_end, next) =
                match fetch_collect_page(client, source, page, cursor.as_deref()).await {
                    Ok(page) => page,
                    Err(error) => {
                        report.error = Some(format!("{}采集失败：{error}", source.label()));
                        break 'sources;
                    }
                };
            report.pages += 1;
            report.fetched += items.len();
            let candidates: Vec<Candidate> = items
                .iter()
                .filter_map(|item| {
                    keep_candidate(
                        Candidate::from_feed_item(item, &source.slug(), now, policy.retention_days),
                        &rules,
                        &patterns,
                        &mut report,
                    )
                })
                .collect();
            match store.upsert_recommend_candidates(&candidates) {
                Ok((inserted, updated)) => {
                    report.inserted += inserted;
                    report.updated += updated;
                }
                Err(error) => {
                    report.error = Some(format!("候选池写入失败：{error}"));
                    break 'sources;
                }
            }
            if is_end || items.is_empty() {
                break;
            }
            cursor = next;
            if cursor.is_none()
                && matches!(
                    source,
                    CollectSource::FollowingFeed | CollectSource::Topic { .. }
                )
            {
                break;
            }
        }
    }

    if report.error.is_none() && !report.cancelled {
        match store.prune_recommend_pool(now, policy.max_items) {
            Ok(pruned) => report.pruned = pruned,
            Err(error) => log::warn!("failed to prune recommendation pool: {error}"),
        }
    }
    report
}

async fn fetch_collect_page(
    client: &ZhihuClient,
    source: &CollectSource,
    page: u32,
    cursor: Option<&str>,
) -> zhihu_api::Result<(Vec<FeedItem>, bool, Option<String>)> {
    match source {
        CollectSource::RecommendFeed => {
            let (items, is_end) = client.feed_recommend_items(page + 1).await?;
            Ok((items, is_end, None))
        }
        CollectSource::FollowingFeed => {
            let page = client
                .follow_feed(zhihu_types::FollowFeedKind::Activity, cursor)
                .await?;
            Ok((page.items, page.is_end, page.next))
        }
        CollectSource::Topic { id } => {
            let page = client
                .topic_feed(
                    id,
                    zhihu_types::TopicFeedTab::Discussion,
                    zhihu_types::TopicSort::Essence,
                    cursor,
                )
                .await?;
            Ok((page.items, page.is_end, page.next))
        }
        CollectSource::Member { token } => {
            let (items, is_end) = client.member_answers(token, page * 20).await?;
            Ok((items, is_end, None))
        }
        CollectSource::Daily => Ok((Vec::new(), true, None)),
    }
}

fn keep_candidate(
    candidate: Option<Candidate>,
    rules: &HiddenRules,
    patterns: &[(String, bool)],
    report: &mut CollectReport,
) -> Option<Candidate> {
    let Some(candidate) = candidate else {
        report.skipped += 1;
        return None;
    };
    if rules.hides(&candidate)
        || is_blocked_text(&candidate.title, patterns)
        || is_blocked_text(&candidate.excerpt, patterns)
    {
        report.skipped += 1;
        return None;
    }
    Some(candidate)
}

fn candidate_embedding_text(candidate: &Candidate) -> String {
    let mut text = format!("{} {}", candidate.title, candidate.excerpt);
    if !candidate.topics.is_empty() {
        text.push(' ');
        text.push_str(&candidate.topics.join(" "));
    }
    text.chars().take(512).collect()
}

/// Embed missing pool items and build the decayed feedback center.
///
/// Runs on a blocking worker when started from [`AppServices::start_recommend_embedding`];
/// the synchronous version is kept for tests and one-shot callers.
fn build_vector_profile(store: &Store, provider: &dyn EmbeddingProvider) -> Option<VectorProfile> {
    let model_id = provider.model_id().to_string();
    let now = unix_now();
    let pool = store.recommend_pool(2000, now).ok()?;
    if pool.is_empty() {
        return None;
    }
    let mut cached: HashMap<String, Vec<f32>> = store
        .recommend_embeddings(&model_id)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let missing: Vec<(String, String)> = pool
        .iter()
        .map(|candidate| {
            (
                item_key(candidate.kind.as_str(), &candidate.id),
                candidate_embedding_text(candidate),
            )
        })
        .filter(|(key, _)| !cached.contains_key(key))
        .take(MAX_EMBED_PER_CALL)
        .collect();
    if !missing.is_empty() {
        let texts: Vec<String> = missing.iter().map(|(_, text)| text.clone()).collect();
        if let Ok(vectors) = provider.embed_batch(&texts) {
            let entries: Vec<(String, Vec<f32>)> = missing
                .into_iter()
                .zip(vectors)
                .map(|((key, _), vector)| (key, vector))
                .collect();
            if store
                .upsert_recommend_embeddings(&model_id, &entries, now)
                .is_ok()
            {
                cached.extend(entries);
            }
        }
    }
    let _ = store.prune_recommend_embeddings(&model_id);

    let mut center = vec![0.0f32; provider.dimension()];
    for (item_id, weight, created_at) in store.recommend_feedback_signals(2000).unwrap_or_default()
    {
        if weight == 0.0 {
            continue;
        }
        let age_days = ((now - created_at).max(0) as f32) / 86_400.0;
        let decayed = weight * 0.5f32.powf(age_days / FEEDBACK_HALF_LIFE_DAYS);
        let Some(vector) = cached.get(&item_id) else {
            continue;
        };
        if vector.len() != center.len() {
            continue;
        }
        for (slot, value) in center.iter_mut().zip(vector) {
            *slot += decayed * value;
        }
    }
    normalize_vector(&mut center);
    Some(VectorProfile::new(model_id, center, cached))
}

fn load_hidden_rules(store: &Store) -> HiddenRules {
    let mut user_ids: Vec<String> = store
        .blocked_users()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    user_ids.extend(
        store
            .recommend_hidden_users()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id),
    );
    let topic_names = store
        .blocked_topics()
        .unwrap_or_default()
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    let hide_mcn = store
        .get_setting("hide_mcn")
        .ok()
        .flatten()
        .map(|value| value != "0")
        .unwrap_or(false);
    HiddenRules {
        user_ids,
        topic_names,
        hide_mcn,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn content_kind_key(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Article => "article",
        ContentKind::Pin => "pin",
        _ => "answer",
    }
}

fn cache_detail(store: &Store, detail: &ContentDetail) {
    let Ok(payload) = serde_json::to_string(detail) else {
        return;
    };
    if let Err(error) = store.cache_content(
        content_kind_key(detail.kind),
        &detail.id,
        &payload,
        unix_now(),
    ) {
        log::warn!("failed to cache content: {error}");
    }
}

fn cached_detail(store: &Store, kind: ContentKind, id: &str) -> Option<ContentDetail> {
    let (payload, _) = store
        .cached_content(content_kind_key(kind), id)
        .ok()
        .flatten()?;
    serde_json::from_str(&payload).ok()
}

fn cached_feed(store: &Store) -> Option<Vec<FeedItem>> {
    let (payload, _) = store.cached_feed("recommend").ok().flatten()?;
    serde_json::from_str(&payload)
        .ok()
        .filter(|items: &Vec<FeedItem>| !items.is_empty())
}

/// Last hot-search snapshot, if the store has one.
fn cached_hot(store: &Store) -> Option<Vec<HotSearchItem>> {
    let (payload, _) = store.cached_feed("hot").ok().flatten()?;
    serde_json::from_str(&payload)
        .ok()
        .filter(|items: &Vec<HotSearchItem>| !items.is_empty())
}

/// Last Zhihu Daily page, if the store has one.
fn cached_daily(store: &Store) -> Option<Vec<DailyStory>> {
    let (payload, _) = store.cached_feed("daily").ok().flatten()?;
    serde_json::from_str(&payload)
        .ok()
        .filter(|items: &Vec<DailyStory>| !items.is_empty())
}

fn record_read(store: &Store, detail: &ContentDetail) {
    let target_type = content_kind_key(detail.kind);
    let now = unix_now();
    if let Err(error) = store.record_read(target_type, &detail.id, &detail.title, now) {
        log::warn!("failed to record read history: {error}");
    }
}

/// Extract the unhuman (risk-control) message and verification URL from an
/// API error string such as `unexpected status 403: {"error":{...}}`.
fn parse_qr_risk(error: &str) -> Option<(String, Option<String>)> {
    let start = error.find('{')?;
    let value: serde_json::Value = serde_json::from_str(&error[start..]).ok()?;
    let object = value.get("error").unwrap_or(&value);
    let code = object.get("code").and_then(serde_json::Value::as_i64);
    let need_login = object
        .get("need_login")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if code != Some(40352) && !need_login {
        return None;
    }
    let message = object
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("知乎检测到当前网络环境异常，需要先完成验证")
        .to_string();
    let redirect = object
        .get("redirect")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Some((message, redirect))
}

/// Posters kept on disk for the cold-start rotation. Each cold start shows one
/// and drops it once the splash is gone, so five is a handful of launches of
/// cover inside a bounded footprint (Bing photos are roughly 200–500 KB each).
const SPLASH_MAX_CARDS: usize = 5;
/// Consumed image URLs remembered so the same photo is never downloaded twice
/// inside the rotation; Bing only offers the last eight days, so the window
/// matches that and old photos become eligible again after a full cycle.
const SPLASH_SEEN_URLS: usize = 8;
const SPLASH_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// One cached poster plus the quote shown over it.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct SplashEntry {
    /// File name inside the splash directory.
    image: String,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    source: String,
    /// Source image URL; empty for cards migrated from the old single file.
    #[serde(default)]
    url: String,
    /// Already shown once; deleted at the next idle cleanup.
    #[serde(default)]
    shown: bool,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct SplashIndex {
    #[serde(default)]
    cards: Vec<SplashEntry>,
    /// Recently consumed URLs, oldest first.
    #[serde(default)]
    seen: Vec<String>,
}

fn load_splash_index(dir: &Path) -> SplashIndex {
    let mut index: SplashIndex = std::fs::read_to_string(dir.join("index.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if index.cards.is_empty() {
        // Migrate the single `card.json` layout written by older builds.
        if let Ok(raw) = std::fs::read_to_string(dir.join("card.json")) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(image) = value.get("image").and_then(|item| item.as_str()) {
                    index.cards.push(SplashEntry {
                        image: image.to_string(),
                        quote: value
                            .get("quote")
                            .and_then(|item| item.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        source: value
                            .get("source")
                            .and_then(|item| item.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        ..Default::default()
                    });
                }
            }
            let _ = std::fs::remove_file(dir.join("card.json"));
        }
    }
    index.cards.retain(|card| dir.join(&card.image).is_file());
    index
}

fn save_splash_index(dir: &Path, index: &SplashIndex) {
    if let Ok(raw) = serde_json::to_string(index) {
        let _ = std::fs::write(dir.join("index.json"), raw);
    }
}

/// Drop files the index no longer references (interrupted writes, old layout).
fn prune_splash_dir(dir: &Path, index: &SplashIndex) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let referenced: HashSet<&str> = index.cards.iter().map(|card| card.image.as_str()).collect();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "index.json" || referenced.contains(name.as_ref()) {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

fn remember_splash_url(seen: &mut Vec<String>, url: &str) {
    if url.is_empty() {
        return;
    }
    if let Some(position) = seen.iter().position(|entry| entry == url) {
        seen.remove(position);
    }
    seen.push(url.to_string());
    let excess = seen.len().saturating_sub(SPLASH_SEEN_URLS);
    if excess > 0 {
        seen.drain(..excess);
    }
}

/// Delete every poster that was already shown, together with its file.
fn remove_shown_splash(dir: &Path, lock: &Mutex<()>) {
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let mut index = load_splash_index(dir);
    let cards = std::mem::take(&mut index.cards);
    let mut kept = Vec::with_capacity(cards.len());
    for card in cards {
        if card.shown {
            let _ = std::fs::remove_file(dir.join(&card.image));
            remember_splash_url(&mut index.seen, &card.url);
        } else {
            kept.push(card);
        }
    }
    index.cards = kept;
    prune_splash_dir(dir, &index);
    save_splash_index(dir, &index);
}

/// Fill the pool up to [`SPLASH_MAX_CARDS`] and return the first poster added.
async fn refill_splash(
    client: &reqwest::Client,
    dir: &Path,
    lock: &Mutex<()>,
) -> Option<SplashCard> {
    // Snapshot the pool without holding the lock over the network calls.
    let (deficit, taken) = {
        let _guard = lock.lock().ok()?;
        let index = load_splash_index(dir);
        let taken: Vec<String> = index
            .cards
            .iter()
            .map(|card| card.url.clone())
            .chain(index.seen.iter().cloned())
            .collect();
        (SPLASH_MAX_CARDS.saturating_sub(index.cards.len()), taken)
    };
    if deficit == 0 {
        return None;
    }
    let fetched = fetch_splash_batch(client, deficit, &taken).await;

    let mut installed: Vec<SplashCard> = Vec::new();
    {
        let _guard = lock.lock().ok()?;
        let mut index = load_splash_index(dir);
        let mut remaining = SPLASH_MAX_CARDS.saturating_sub(index.cards.len());
        let stamp = splash_stamp();
        for (position, (url, bytes, quote, source)) in fetched.into_iter().enumerate() {
            if remaining == 0 {
                break;
            }
            if index.cards.iter().any(|card| card.url == url) {
                continue;
            }
            let image = format!("poster-{stamp}-{position}.jpg");
            if std::fs::write(dir.join(&image), &bytes).is_err() {
                continue;
            }
            let entry = SplashEntry {
                image,
                quote,
                source,
                url,
                shown: false,
            };
            installed.push(SplashCard {
                image_path: dir.join(&entry.image),
                quote: entry.quote.clone(),
                source: entry.source.clone(),
            });
            index.cards.push(entry);
            remaining -= 1;
        }
        while index.cards.len() > SPLASH_MAX_CARDS {
            // Over the cap: overwrite the posters already seen first.
            let position = index.cards.iter().position(|card| card.shown).unwrap_or(0);
            let card = index.cards.remove(position);
            let _ = std::fs::remove_file(dir.join(&card.image));
            remember_splash_url(&mut index.seen, &card.url);
        }
        prune_splash_dir(dir, &index);
        save_splash_index(dir, &index);
    }
    installed.into_iter().next()
}

/// Download up to `need` unseen posters (Bing publishes the last eight days),
/// pairing each with a 一言 quote. Returns `(url, bytes, quote, source)`.
async fn fetch_splash_batch(
    client: &reqwest::Client,
    need: usize,
    taken: &[String],
) -> Vec<(String, Vec<u8>, String, String)> {
    const BING_URL: &str = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=zh-CN";
    let mut candidates = Vec::new();
    if let Ok(response) = client
        .get(BING_URL)
        .header(reqwest::header::USER_AGENT, SPLASH_UA)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
    {
        if let Ok(text) = response.text().await {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(images) = value.get("images").and_then(|item| item.as_array()) {
                    for image in images {
                        let Some(url) = image.get("url").and_then(|item| item.as_str()) else {
                            continue;
                        };
                        let url = if url.starts_with("http") {
                            url.to_string()
                        } else {
                            format!("https://www.bing.com{url}")
                        };
                        if taken.iter().any(|used| used == &url) {
                            continue;
                        }
                        candidates.push(url);
                        if candidates.len() >= need {
                            break;
                        }
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        // Every photo in the Bing window was already seen: a random picture
        // keeps the rotation alive until Bing publishes a new day.
        candidates.push(format!(
            "https://picsum.photos/1080/1920?random={}",
            splash_stamp()
        ));
    }

    let mut batch = Vec::new();
    for url in candidates {
        let Ok(response) = client
            .get(&url)
            .header(reqwest::header::USER_AGENT, SPLASH_UA)
            .timeout(std::time::Duration::from_secs(12))
            .send()
            .await
        else {
            continue;
        };
        let Ok(response) = response.error_for_status() else {
            continue;
        };
        let Ok(bytes) = response.bytes().await else {
            continue;
        };
        if bytes.len() < 1024 || bytes.len() > 6 * 1024 * 1024 {
            continue;
        }
        let (quote, source) = fetch_splash_quote(client).await.unwrap_or_default();
        batch.push((url, bytes.to_vec(), quote, source));
    }
    batch
}

/// One 一言 sentence and its attribution.
async fn fetch_splash_quote(client: &reqwest::Client) -> Option<(String, String)> {
    const QUOTE_URL: &str = "https://v1.hitokoto.cn/?c=d&c=i&c=k";
    let text = client
        .get(QUOTE_URL)
        .header(reqwest::header::USER_AGENT, SPLASH_UA)
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    let quote: serde_json::Value = serde_json::from_str(&text).ok()?;
    let sentence = quote.get("hitokoto")?.as_str()?.trim().to_string();
    if sentence.is_empty() {
        return None;
    }
    let from = quote
        .get("from")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    let who = quote
        .get("from_who")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    let source = match (who.is_empty(), from.is_empty()) {
        (false, false) => format!("— {who}《{from}》"),
        (false, true) => format!("— {who}"),
        (true, false) => format!("— {from}"),
        _ => String::new(),
    };
    Some((sentence, source))
}

fn splash_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

fn prune_image_cache(dir: &Path, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().unwrap_or(std::time::UNIX_EPOCH), path))
        })
        .collect();
    if files.len() <= max_files {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let excess = files.len().saturating_sub(max_files);
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

fn filter_feed_items(items: Vec<FeedItem>, store: &Store) -> Vec<FeedItem> {
    let patterns = store.block_keywords().unwrap_or_default();
    let users = store.blocked_users().unwrap_or_default();
    let topics = store.blocked_topics().unwrap_or_default();
    items
        .into_iter()
        .filter(|item| {
            !is_blocked_text(
                &format!("{} {} {}", item.title, item.excerpt, item.author.name),
                &patterns,
            )
        })
        .filter(|item| {
            !users
                .iter()
                .any(|(id, name)| item.author.id == *id || item.author.name == *name)
        })
        .filter(|item| {
            !topics
                .iter()
                .any(|(id, name)| item.topics.iter().any(|topic| topic == id || topic == name))
        })
        .collect()
}

fn is_blocked_text(text: &str, patterns: &[(String, bool)]) -> bool {
    patterns.iter().any(|(pattern, is_regex)| {
        if *is_regex {
            regex::Regex::new(pattern)
                .map(|regex| regex.is_match(text))
                .unwrap_or(false)
        } else {
            text.contains(pattern)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qr_risk_response() {
        let error = r#"unexpected status 403: {"error":{"need_login":true,"redirect":"https://www.zhihu.com/account/unhuman?type=S6E3V1","code":40352,"message":"系统监测到您的网络环境存在异常"}}"#;
        let (message, redirect) = parse_qr_risk(error).expect("risk body");
        assert_eq!(message, "系统监测到您的网络环境存在异常");
        assert_eq!(
            redirect.as_deref(),
            Some("https://www.zhihu.com/account/unhuman?type=S6E3V1")
        );
        assert!(parse_qr_risk("unexpected status 500: {}").is_none());
        assert!(parse_qr_risk("network unreachable").is_none());
    }

    #[test]
    fn offline_cache_helpers_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let detail = ContentDetail {
            id: "7".to_string(),
            title: "cached".to_string(),
            author: zhihu_types::Author {
                id: "u1".to_string(),
                name: "alice".to_string(),
                headline: None,
                url_token: None,
                is_following: false,
                avatar_url: None,
                is_mcn: false,
            },
            kind: ContentKind::Answer,
            content_html: "<p>offline</p>".to_string(),
            excerpt: String::new(),
            vote_up_count: 1,
            comment_count: 0,
            vote_state: "neutral".to_string(),
            created_time: 0,
            updated_time: 0,
            question_id: None,
        };
        cache_detail(&store, &detail);
        assert_eq!(
            cached_detail(&store, ContentKind::Answer, "7")
                .unwrap()
                .title,
            "cached"
        );
        assert!(cached_detail(&store, ContentKind::Article, "7").is_none());

        let items = vec![zhihu_types::m0_sample_feed()[0].clone()];
        store
            .cache_feed("recommend", &serde_json::to_string(&items).unwrap(), 100)
            .unwrap();
        assert_eq!(cached_feed(&store).unwrap()[0].id, items[0].id);
        assert!(cached_feed(&Store::open_in_memory().unwrap()).is_none());
    }

    #[test]
    fn app_settings_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let settings = AppSettings {
            dark: false,
            font_size: 19.0,
            line_height: 32.0,
            tab: 4,
            tts_rate: 1.3,
            tts_pitch: 1.1,
            tts_volume: 0.7,
            tts_voice: "voice-a".to_string(),
            recommend_mode: RecommendMode::Hybrid,
            recommend_preset: RecommendPreset::Explore,
            typography: "relaxed".to_string(),
            reading_indent: false,
            pos_colors: false,
            hide_mcn: true,
        };
        settings.save(&store);
        let loaded = AppSettings::load(&store);
        assert_eq!(loaded.dark, settings.dark);
        assert!(!loaded.reading_indent);
        assert!(!loaded.pos_colors);
        assert!(loaded.hide_mcn);
        assert_eq!(loaded.font_size, settings.font_size);
        assert_eq!(loaded.line_height, settings.line_height);
        assert_eq!(loaded.recommend_mode, RecommendMode::Hybrid);
        assert_eq!(loaded.recommend_preset, RecommendPreset::Explore);
        assert_eq!(loaded.tab, 4);
        assert_eq!(loaded.recommend_mode, RecommendMode::Hybrid);
        assert!((loaded.tts_rate - 1.3).abs() < 0.01);
        assert!((loaded.tts_volume - 0.7).abs() < 0.01);
        assert_eq!(loaded.tts_voice, "voice-a");
        assert_eq!(loaded.typography, "relaxed");
    }

    #[test]
    fn blocklist_matches_substring_and_regex() {
        assert!(is_blocked_text("这是一条广告", &[("广告".into(), false)]));
        assert!(!is_blocked_text("正常内容", &[("广告".into(), false)]));
        assert!(is_blocked_text("推广一下", &[("^推广".into(), true)]));
        assert!(!is_blocked_text("不是推广", &[("^推广".into(), true)]));
    }

    #[test]
    fn blocklist_persists_via_services() {
        let dir = std::env::temp_dir().join(format!("zhihu-block-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let services = AppServices::new(&dir).unwrap();
        services.add_block_keyword("rust".to_string(), false);
        let keywords = services.block_keywords();
        assert!(keywords.iter().any(|(pattern, _)| pattern == "rust"));
        services.remove_block_keyword("rust".to_string());
        assert!(services.block_keywords().is_empty());
        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collector_skips_hidden_and_blocked_candidates() {
        let mut report = CollectReport::default();
        let rules = HiddenRules {
            user_ids: vec!["blocked-user".to_string()],
            topic_names: vec!["屏蔽话题".to_string()],
            hide_mcn: true,
        };
        let patterns = vec![("广告".to_string(), false)];
        let items = [
            collect_item("1", "u1", "正常内容", "Rust", false),
            collect_item("2", "blocked-user", "正常内容", "Rust", false),
            collect_item("3", "u2", "这是一条广告", "Rust", false),
            collect_item("4", "u3", "正常内容", "屏蔽话题", false),
            collect_item("5", "u4", "正常内容", "Rust", true),
            collect_item(" ", "u5", "正常内容", "Rust", false),
        ];
        let kept: Vec<Candidate> = items
            .iter()
            .filter_map(|item| {
                keep_candidate(
                    Candidate::from_feed_item(item, "recommend", 100, 7),
                    &rules,
                    &patterns,
                    &mut report,
                )
            })
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "1");
        assert_eq!(kept[0].source, "recommend");
        assert_eq!(kept[0].expires_at, 100 + 7 * 86_400);
        assert_eq!(report.skipped, 5);
    }

    #[test]
    fn hidden_rules_merge_every_blocklist() {
        let store = Store::open_in_memory().unwrap();
        store.add_block_user("u1", "one").unwrap();
        store.add_recommend_hidden_user("u2", "two").unwrap();
        store.add_block_topic("t1", "Rust").unwrap();
        store.set_setting("hide_mcn", "1").unwrap();

        let rules = load_hidden_rules(&store);
        assert_eq!(rules.user_ids, vec!["u1".to_string(), "u2".to_string()]);
        assert_eq!(rules.topic_names, vec!["Rust".to_string()]);
        assert!(rules.hide_mcn);
    }

    #[test]
    fn recommend_vector_profile_embeds_pool_and_feedback() {
        let dir = std::env::temp_dir().join(format!("zhihu-vector-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let services = AppServices::new(&dir).unwrap();
        let now = unix_now();
        let candidate = |id: &str, title: &str| Candidate {
            kind: zhihu_recommend::CandidateKind::Answer,
            id: id.to_string(),
            title: title.to_string(),
            excerpt: String::new(),
            author_id: "u1".to_string(),
            author_name: "alice".to_string(),
            topics: vec!["Rust".to_string()],
            vote_up_count: 1,
            comment_count: 0,
            is_mcn: false,
            source: "recommend".to_string(),
            collected_at: now - 60,
            expires_at: now + 7 * 86_400,
        };
        services
            .store()
            .upsert_recommend_candidates(&[
                candidate("1", "Rust 移动端实践"),
                candidate("2", "今天中午吃什么"),
            ])
            .unwrap();
        services.record_recommend_feedback("answer:1", "like", "u1", &[], 2.0);

        let profile = services.recommend_vector_profile().expect("vector profile");
        assert!(!profile.model_id.is_empty());
        assert_eq!(profile.model_id, services.embedding_model_id());
        assert_eq!(profile.item_count(), 2);
        assert!(profile.contains("answer", "1"));
        assert!(!profile.contains("answer", "9"));
        assert!(profile.similarity("answer", "1").is_some());
        assert_eq!(services.recommend_embedding_count(), 2);

        // A second call reuses the cached rows.
        let again = services.recommend_vector_profile().unwrap();
        assert_eq!(again.item_count(), 2);
        assert_eq!(services.recommend_embedding_count(), 2);

        services.clear_recommend_pool();
        assert_eq!(services.recommend_embedding_count(), 0);
        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn collect_item(id: &str, author: &str, title: &str, topic: &str, is_mcn: bool) -> FeedItem {
        FeedItem {
            id: id.to_string(),
            title: title.to_string(),
            excerpt: String::new(),
            author: zhihu_types::Author {
                id: author.to_string(),
                name: author.to_string(),
                headline: None,
                url_token: None,
                is_following: false,
                avatar_url: None,
                is_mcn,
            },
            kind: ContentKind::Answer,
            vote_up_count: 1,
            comment_count: 0,
            created_at: 0,
            topics: vec![topic.to_string()],
            topic_refs: Vec::new(),
        }
    }

    #[test]
    fn collection_requires_explicit_opt_in() {
        let dir =
            std::env::temp_dir().join(format!("zhihu-rust-collect-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let services = AppServices::new(&dir).unwrap();
        assert!(!services.recommend_collection_enabled());
        assert_eq!(services.recommend_pool_stats(), Default::default());

        let _token = services.start_recommend_collection(CollectRequest::default());
        let event = services.receiver().try_recv().expect("opt-in error event");
        match event {
            UiEvent::RecommendCollected(report) => {
                assert!(!report.succeeded());
                assert_eq!(report.error.as_deref(), Some("本地推荐采集未开启"));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        services.set_recommend_collection_enabled(true);
        assert!(services.recommend_collection_enabled());
        services.set_recommend_collection_enabled(false);
        assert!(!services.recommend_collection_enabled());
        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn background_embedding_reports_a_ready_profile() {
        let dir = std::env::temp_dir().join(format!("zhihu-embed-job-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let services = AppServices::new(&dir).unwrap();
        let now = unix_now();
        services
            .store()
            .upsert_recommend_candidates(&[Candidate {
                kind: zhihu_recommend::CandidateKind::Answer,
                id: "1".to_string(),
                title: "Rust 移动端实践".to_string(),
                excerpt: String::new(),
                author_id: "u1".to_string(),
                author_name: "alice".to_string(),
                topics: vec!["Rust".to_string()],
                vote_up_count: 1,
                comment_count: 0,
                is_mcn: false,
                source: "recommend".to_string(),
                collected_at: now - 30,
                expires_at: now + 86_400,
            }])
            .unwrap();
        assert!(services.start_recommend_embedding());

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut ready = None;
        while Instant::now() < deadline {
            match services.receiver().try_recv() {
                Ok(UiEvent::RecommendEmbeddings(event)) => {
                    ready = Some(event);
                    break;
                }
                Ok(_) => continue,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        let ready = ready.expect("background embedding event");
        assert!(ready.error.is_none(), "{:?}", ready.error);
        let profile = ready.profile.expect("profile with one item");
        assert_eq!(profile.item_count(), 1);
        assert_eq!(services.recommend_embedding_count(), 1);
        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cookie_import_is_persisted() {
        let dir = std::env::temp_dir().join(format!("zhihu-rust-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let services = AppServices::new(&dir).unwrap();
        assert!(!services.is_logged_in());
        let count = services.import_cookie_header("z_c0=fake; d_c0=abc");
        assert_eq!(count, 2);
        assert!(services.is_logged_in());
        assert_eq!(services.store().load_cookies(0).unwrap().len(), 2);
        drop(services);

        let reopened = AppServices::new(&dir).unwrap();
        assert!(reopened.is_logged_in());
        drop(reopened);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod splash_pool_tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zhihu-splash-{}-{}-{name}",
            std::process::id(),
            splash_stamp()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_card(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), vec![0u8; 2048]).unwrap();
    }

    fn index_with(cards: Vec<SplashEntry>) -> SplashIndex {
        SplashIndex {
            cards,
            seen: Vec::new(),
        }
    }

    fn entry(name: &str, url: &str, shown: bool) -> SplashEntry {
        SplashEntry {
            image: name.to_string(),
            quote: "q".to_string(),
            source: "s".to_string(),
            url: url.to_string(),
            shown,
        }
    }

    #[test]
    fn migrates_the_legacy_single_card_file() {
        let dir = temp_dir("legacy");
        write_card(&dir, "poster.jpg");
        std::fs::write(
            dir.join("card.json"),
            r#"{"image":"poster.jpg","quote":"旧句子","source":"— 旧"}"#,
        )
        .unwrap();

        let index = load_splash_index(&dir);
        assert_eq!(index.cards.len(), 1);
        assert_eq!(index.cards[0].image, "poster.jpg");
        assert_eq!(index.cards[0].quote, "旧句子");
        assert!(!index.cards[0].shown);
        assert!(!dir.join("card.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entries_without_a_file_are_dropped() {
        let dir = temp_dir("missing");
        write_card(&dir, "keep.jpg");
        save_splash_index(
            &dir,
            &index_with(vec![
                entry("keep.jpg", "u1", false),
                entry("gone.jpg", "u2", false),
            ]),
        );

        let index = load_splash_index(&dir);
        assert_eq!(index.cards.len(), 1);
        assert_eq!(index.cards[0].image, "keep.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_shown_posters_and_remembers_their_urls() {
        let dir = temp_dir("cleanup");
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            write_card(&dir, name);
        }
        save_splash_index(
            &dir,
            &index_with(vec![
                entry("a.jpg", "ua", true),
                entry("b.jpg", "ub", false),
                entry("c.jpg", "uc", true),
            ]),
        );

        let lock = Mutex::new(());
        remove_shown_splash(&dir, &lock);

        let index = load_splash_index(&dir);
        assert_eq!(index.cards.len(), 1);
        assert_eq!(index.cards[0].image, "b.jpg");
        assert!(!dir.join("a.jpg").exists());
        assert!(!dir.join("c.jpg").exists());
        assert!(dir.join("b.jpg").exists());
        assert_eq!(index.seen, vec!["ua".to_string(), "uc".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seen_urls_form_a_bounded_ring() {
        let mut seen = Vec::new();
        for index in 0..(SPLASH_SEEN_URLS + 3) {
            remember_splash_url(&mut seen, &format!("u{index}"));
        }
        assert_eq!(seen.len(), SPLASH_SEEN_URLS);
        assert_eq!(seen.first().map(String::as_str), Some("u3"));
        // Re-visiting a URL moves it to the back instead of duplicating it.
        remember_splash_url(&mut seen, "u5");
        assert_eq!(seen.last().map(String::as_str), Some("u5"));
        assert_eq!(seen.iter().filter(|url| *url == "u5").count(), 1);
    }

    #[test]
    fn pruning_prefers_overwriting_already_shown_posters() {
        let dir = temp_dir("cap");
        let mut cards = Vec::new();
        for position in 0..(SPLASH_MAX_CARDS + 1) {
            let name = format!("p{position}.jpg");
            write_card(&dir, &name);
            // The second card was shown already; it should be the one dropped.
            cards.push(entry(&name, &format!("u{position}"), position == 1));
        }
        let mut index = index_with(cards);
        while index.cards.len() > SPLASH_MAX_CARDS {
            let position = index.cards.iter().position(|card| card.shown).unwrap_or(0);
            let card = index.cards.remove(position);
            let _ = std::fs::remove_file(dir.join(&card.image));
            remember_splash_url(&mut index.seen, &card.url);
        }
        assert_eq!(index.cards.len(), SPLASH_MAX_CARDS);
        assert!(index.cards.iter().all(|card| card.image != "p1.jpg"));
        assert!(!dir.join("p1.jpg").exists());
        assert_eq!(index.seen, vec!["u1".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphaned_files_are_pruned() {
        let dir = temp_dir("orphan");
        write_card(&dir, "referenced.jpg");
        write_card(&dir, "stray.jpg");
        save_splash_index(&dir, &index_with(vec![entry("referenced.jpg", "u", false)]));

        let index = load_splash_index(&dir);
        prune_splash_dir(&dir, &index);
        assert!(dir.join("referenced.jpg").exists());
        assert!(!dir.join("stray.jpg").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
