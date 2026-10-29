//! Headless application core: services, routing and view models.
//!
//! This crate deliberately does not depend on GPUI so it can be exercised with
//! plain `cargo test`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tokio::sync::Mutex as AsyncMutex;
use zhihu_api::{PhoneLoginSession, ZhihuClient};
pub use zhihu_api::{SearchOptions, SearchSort, SearchVertical};
use zhihu_recommend::{FeedbackRecord, RecommendMode};
use zhihu_store::{Store, DB_FILE_NAME};
use zhihu_types::{
    AccountProfile, Comment, ContentDetail, ContentKind, DailyStory, FeedItem, HistoryEntry,
    HotSearchItem, NotificationEntry,
};

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
    /// Typography preset key: `standard`, `relaxed`, `compact` or `custom`.
    pub typography: String,
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
            typography: "standard".to_string(),
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
        settings
    }

    pub fn save(&self, store: &Store) {
        let _ = store.set_setting("theme", if self.dark { "dark" } else { "light" });
        let _ = store.set_setting("font_size", &self.font_size.to_string());
        let _ = store.set_setting("line_height", &self.line_height.to_string());
        let _ = store.set_setting("tab", &self.tab.to_string());
        let _ = store.set_setting("recommend_mode", self.recommend_mode.as_str());
        let _ = store.set_setting("tts_rate", &self.tts_rate.to_string());
        let _ = store.set_setting("tts_pitch", &self.tts_pitch.to_string());
        let _ = store.set_setting("tts_volume", &self.tts_volume.to_string());
        let _ = store.set_setting("tts_voice", &self.tts_voice);
        let _ = store.set_setting("typography", &self.typography);
    }
}

#[derive(Debug)]
pub enum UiEvent {
    HotSearch(Vec<HotSearchItem>),
    Daily(Vec<DailyStory>),
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
    Detail(ContentDetail),
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
    Member(zhihu_types::MemberProfile),
    MemberError(String),
    MemberAnswers {
        items: Vec<FeedItem>,
        append: bool,
        is_end: bool,
    },
    MemberAnswersError(String),
    MemberFollowed(bool),
    CommentLikeUpdated {
        comment_id: String,
        liked: bool,
    },
    CommentLikeError(String),
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
        items: Vec<Comment>,
        append: bool,
        is_end: bool,
    },
    CommentsError(String),
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
        append: bool,
        is_end: bool,
    },
    NotificationsError(String),
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
    Error(String),
}

/// Network runtime shared by the UI.
///
/// Network requests run on a Tokio runtime owned by this struct; results are
/// delivered to the GPUI thread through an `async_channel`.
pub struct AppServices {
    client: Arc<ZhihuClient>,
    store: Arc<Store>,
    image_dir: PathBuf,
    image_client: reqwest::Client,
    pending_images: Arc<Mutex<HashSet<String>>>,
    phone_login: Arc<AsyncMutex<Option<PhoneLoginSession>>>,
    sender: async_channel::Sender<UiEvent>,
    receiver: async_channel::Receiver<UiEvent>,
    runtime: tokio::runtime::Runtime,
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
        let image_client = reqwest::Client::builder()
            .user_agent(zhihu_api::DEFAULT_USER_AGENT)
            .build()?;

        let (sender, receiver) = async_channel::unbounded();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(Self {
            client,
            store,
            image_dir,
            image_client,
            pending_images: Arc::new(Mutex::new(HashSet::new())),
            phone_login: Arc::new(AsyncMutex::new(None)),
            sender,
            receiver,
            runtime,
        })
    }

    pub fn client(&self) -> Arc<ZhihuClient> {
        self.client.clone()
    }

    pub fn receiver(&self) -> async_channel::Receiver<UiEvent> {
        self.receiver.clone()
    }

    pub fn refresh_hot_search(&self) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.hot_search().await {
                Ok(items) => {
                    let patterns = store.block_keywords().unwrap_or_default();
                    UiEvent::HotSearch(
                        items
                            .into_iter()
                            .filter(|item| !is_blocked_text(&item.query, &patterns))
                            .collect(),
                    )
                }
                Err(error) => UiEvent::Error(format!("热榜加载失败：{error}")),
            };
            let _ = sender.send(event).await;
        });
    }

    pub fn refresh_daily(&self) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.daily_latest().await {
                Ok(items) => {
                    let patterns = store.block_keywords().unwrap_or_default();
                    UiEvent::Daily(
                        items
                            .into_iter()
                            .filter(|item| !is_blocked_text(&item.title, &patterns))
                            .collect(),
                    )
                }
                Err(error) => UiEvent::Error(format!("日报加载失败：{error}")),
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
                            let _ = sender.send(UiEvent::QrRisk { message, redirect }).await;
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
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.feed_recommend_items(1).await {
                Ok((items, is_end)) => UiEvent::Feed {
                    items: filter_feed_items(items, &store),
                    is_end,
                },
                Err(error) => UiEvent::Error(format!("推荐流加载失败：{error}")),
            };
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

    pub fn load_answer(&self, id: String) {
        let client = self.client.clone();
        let sender = self.sender.clone();
        let store = self.store.clone();
        self.runtime.spawn(async move {
            let event = match client.answer_detail(&id).await {
                Ok(detail) => {
                    record_read(&store, &detail);
                    UiEvent::Detail(detail)
                }
                Err(error) => UiEvent::DetailError(format!("回答加载失败：{error}")),
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
                    record_read(&store, &detail);
                    UiEvent::Detail(detail)
                }
                Err(error) => UiEvent::DetailError(format!("文章加载失败：{error}")),
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
                    items,
                    append,
                    is_end,
                },
                Err(error) => UiEvent::MemberAnswersError(format!("用户回答加载失败：{error}")),
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

    pub fn blocked_users(&self) -> Vec<(String, String)> {
        self.store.blocked_users().unwrap_or_default()
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
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::NotificationsError(format!("通知加载失败：{error}")),
            };
            let _ = sender.send(event).await;
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
        self.runtime.spawn(async move {
            let event = match client.comments(kind, &id, offset).await {
                Ok(page) => UiEvent::CommentsPage {
                    items: page.items,
                    append,
                    is_end: page.is_end,
                },
                Err(error) => UiEvent::CommentsError(format!("评论加载失败：{error}")),
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

fn record_read(store: &Store, detail: &ContentDetail) {
    let target_type = match detail.kind {
        ContentKind::Answer => "answer",
        ContentKind::Article => "article",
        _ => "answer",
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
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

/// Keep the on-disk image cache bounded; oldest files are evicted first.
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
            typography: "relaxed".to_string(),
        };
        settings.save(&store);
        let loaded = AppSettings::load(&store);
        assert_eq!(loaded.dark, settings.dark);
        assert_eq!(loaded.font_size, settings.font_size);
        assert_eq!(loaded.line_height, settings.line_height);
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
