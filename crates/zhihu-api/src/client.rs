//! Minimal Zhihu HTTP client.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, REFERER, USER_AGENT};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use url::Url;
use zhihu_types::{DailyStory, HotSearchItem};

use crate::sign::{self, ZHIHU_WEB_ZSE93};

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

const HOME_URL: &str = "https://www.zhihu.com/";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Clone, Debug)]
pub struct QrLoginSession {
    pub link: String,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug)]
pub struct QrScanStatus {
    pub http_status: u16,
    pub qr_status: i64,
    pub login_status: String,
    pub success: bool,
    pub message: Option<String>,
    pub redirect: Option<String>,
    pub risk_control: bool,
}

const SIGNIN_REFERER: &str = "https://www.zhihu.com/signin";
const SIGNIN_URL: &str = "https://www.zhihu.com/signin?next=%2F";

/// Search content type (`vertical` parameter of `search_v3`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchVertical {
    #[default]
    All,
    Answer,
    Article,
    Video,
    Question,
}

impl SearchVertical {
    pub fn parameter(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Answer => Some("answer"),
            Self::Article => Some("article"),
            Self::Video => Some("zvideo"),
            Self::Question => Some("question"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "综合",
            Self::Answer => "回答",
            Self::Article => "文章",
            Self::Video => "视频",
            Self::Question => "问题",
        }
    }
}

/// Search ordering (`sort` parameter of `search_v3`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSort {
    #[default]
    Default,
    Latest,
    MostVoted,
}

impl SearchSort {
    pub fn parameter(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Latest => Some("created_time"),
            Self::MostVoted => Some("upvoted_count"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "综合排序",
            Self::Latest => "最新",
            Self::MostVoted => "最多赞同",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub vertical: SearchVertical,
    pub sort: SearchSort,
}

pub struct ZhihuClient {
    http: reqwest::Client,
    jar: Arc<Jar>,
    dc0: Mutex<Option<String>>,
    user_agent: String,
}

impl ZhihuClient {
    pub fn new() -> Result<Self> {
        Self::with_user_agent(DEFAULT_USER_AGENT)
    }

    pub fn with_user_agent(user_agent: &str) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(user_agent).unwrap());
        headers.insert(REFERER, HeaderValue::from_static(HOME_URL));

        let http = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .build()?;

        Ok(Self {
            http,
            jar,
            dc0: Mutex::new(None),
            user_agent: user_agent.to_string(),
        })
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Visit the homepage once so the server can issue `_xsrf` / `_zap` and
    /// (if the front-end/JS sets it) `d_c0`.
    pub async fn bootstrap(&self) -> Result<()> {
        let response = self.http.get(HOME_URL).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let _ = response.text().await?;
        self.refresh_dc0()?;
        Ok(())
    }

    /// Load `d_c0` from the cookie jar; synthesize one if the homepage did not
    /// set it (anonymous web access).
    pub fn refresh_dc0(&self) -> Result<String> {
        if let Some(dc0) = self.cookie_value("d_c0") {
            *self.dc0.lock().unwrap() = Some(dc0.clone());
            return Ok(dc0);
        }

        let dc0 = synthetic_dc0();
        self.set_cookie("d_c0", &dc0);
        *self.dc0.lock().unwrap() = Some(dc0.clone());
        Ok(dc0)
    }

    pub fn dc0(&self) -> Option<String> {
        if let Some(value) = self.dc0.lock().unwrap().clone() {
            return Some(value);
        }
        self.cookie_value("d_c0")
    }

    pub fn cookie_value(&self, name: &str) -> Option<String> {
        let url = Url::parse(HOME_URL).ok()?;
        let header = self.jar.cookies(&url)?;
        let header = header.to_str().ok()?;
        header.split(';').find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_string())
        })
    }

    pub fn all_cookies(&self) -> Vec<(String, String)> {
        self.cookie_header()
            .map(|header| {
                header
                    .split(';')
                    .filter_map(|pair| {
                        let (name, value) = pair.trim().split_once('=')?;
                        Some((name.to_string(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn cookie_header(&self) -> Option<String> {
        let url = Url::parse(HOME_URL).ok()?;
        let header = self.jar.cookies(&url)?;
        header.to_str().ok().map(ToOwned::to_owned)
    }

    pub fn set_cookie(&self, name: &str, value: &str) {
        if let Ok(url) = Url::parse(HOME_URL) {
            self.jar
                .add_cookie_str(&format!("{name}={value}; Domain=.zhihu.com; Path=/"), &url);
        }
    }

    /// Import cookies from a login session or a browser export.
    pub fn set_cookies<I, K, V>(&self, cookies: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (name, value) in cookies {
            self.set_cookie(name.as_ref(), value.as_ref());
        }
        *self.dc0.lock().unwrap() = self.cookie_value("d_c0");
    }

    pub async fn get_json(&self, url: &str, signed: bool) -> Result<Value> {
        self.send_json(Method::GET, url, None, signed).await
    }

    pub async fn get_json_with_referer(
        &self,
        url: &str,
        signed: bool,
        referer: &str,
    ) -> Result<Value> {
        self.send_json_with_referer(Method::GET, url, None, signed, Some(referer))
            .await
    }

    async fn send_json(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        signed: bool,
    ) -> Result<Value> {
        self.send_json_with_referer(method, url, body, signed, None)
            .await
    }

    async fn send_json_with_referer(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        signed: bool,
        referer: Option<&str>,
    ) -> Result<Value> {
        let body_text = body.map(serde_json::to_string).transpose()?;
        let mut request = self.http.request(method, url);
        if let Some(referer) = referer {
            request = request.header(REFERER, referer);
        }
        if let Some(body_text) = &body_text {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body_text.clone());
        }
        if signed {
            let dc0 = match self.dc0() {
                Some(value) => value,
                None => self.refresh_dc0()?,
            };
            request = request
                .header("x-zse-93", ZHIHU_WEB_ZSE93)
                .header(
                    "x-zse-96",
                    sign::zse96(ZHIHU_WEB_ZSE93, url, &dc0, body_text.as_deref()),
                )
                .header("x-requested-with", "fetch");
            if let Some(xsrf) = self.cookie_value("_xsrf") {
                request = request.header("x-xsrftoken", xsrf);
            }
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(ApiError::Status {
                status: status.as_u16(),
                body,
            });
        }
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn feed_recommend(&self, page: u32) -> Result<Value> {
        let url = format!(
            "https://www.zhihu.com/api/v3/feed/topstory/recommend?desktop=true&page_number={page}&limit=10"
        );
        self.get_json(&url, true).await
    }

    /// Recommended feed mapped to domain cards plus the end marker.
    /// Requires login cookies.
    pub async fn feed_recommend_items(
        &self,
        page: u32,
    ) -> Result<(Vec<zhihu_types::FeedItem>, bool)> {
        let value = self.feed_recommend(page).await?;
        Ok(crate::feed::parse_feed_page(&value))
    }

    /// Signed full search (search_v3). Raw JSON.
    pub async fn search_json(
        &self,
        query: &str,
        offset: u32,
        options: SearchOptions,
    ) -> Result<Value> {
        let mut url = Url::parse("https://www.zhihu.com/api/v4/search_v3")
            .map_err(|error| ApiError::Other(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("t", "general")
                .append_pair("q", query)
                .append_pair("correction", "1")
                .append_pair("offset", &offset.to_string())
                .append_pair("limit", "20")
                .append_pair("filter_fields", "")
                .append_pair("lc_idx", "0")
                .append_pair("show_all_topics", "0");
            if let Some(vertical) = options.vertical.parameter() {
                pairs
                    .append_pair("vertical", vertical)
                    .append_pair("vertical_info", "0,0,0,0,0,0,0,0,0,0,0,0");
            }
            if let Some(sort) = options.sort.parameter() {
                pairs.append_pair("sort", sort);
            }
            pairs.append_pair(
                "search_source",
                if options == SearchOptions::default() {
                    "Normal"
                } else {
                    "Filter"
                },
            );
        }
        self.get_json(url.as_str(), true).await
    }

    async fn login_request(
        &self,
        method: Method,
        url: &str,
        body: Option<&str>,
        referer: &str,
        polling: bool,
    ) -> Result<(u16, Value)> {
        let mut request = self
            .http
            .request(method, url)
            .header(
                "Accept",
                if polling { "*/*" } else { "application/json, text/plain, */*" },
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
            )
            .header(
                "sec-ch-ua",
                r#""Not:A-Brand";v="99", "Google Chrome";v="145", "Chromium";v="145""#,
            )
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", r#""Windows""#)
            .header("Origin", "https://www.zhihu.com")
            .header("Referer", referer)
            .header("x-requested-with", "fetch")
            .header("content-type", "application/json;charset=UTF-8");
        if polling {
            request = request.header("x-zse-93", ZHIHU_WEB_ZSE93);
        }
        // The QR endpoints accept the cookie-only form; an incorrect
        // x-xsrftoken makes /login/qrcode return 400 Account.RequestError.
        let _ = self.cookie_value("_xsrf");
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request.send().await?;
        let http_status = response.status().as_u16();
        let text = response.text().await?;
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((http_status, value))
    }

    /// Start a QR login session (anonymous).
    pub async fn qr_login_start(&self) -> Result<QrLoginSession> {
        let _ = self
            .login_request(
                Method::GET,
                SIGNIN_URL,
                None,
                "https://www.zhihu.com/",
                false,
            )
            .await;
        let _ = self
            .login_request(
                Method::POST,
                "https://www.zhihu.com/udid",
                Some("{}"),
                SIGNIN_REFERER,
                false,
            )
            .await;
        let _ = self
            .login_request(
                Method::GET,
                "https://www.zhihu.com/api/v3/oauth/captcha/v2?type=captcha_sign_in",
                None,
                SIGNIN_REFERER,
                false,
            )
            .await;
        let (http_status, value) = self
            .login_request(
                Method::POST,
                "https://www.zhihu.com/api/v3/account/api/login/qrcode",
                Some("{}"),
                SIGNIN_REFERER,
                false,
            )
            .await?;
        let link = value
            .get("link")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let token = value
            .get("token")
            .and_then(Value::as_str)
            .or_else(|| value.get("qrcode_token").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        if http_status >= 400 || link.is_empty() || token.is_empty() {
            return Err(ApiError::Status {
                status: http_status,
                body: value.to_string(),
            });
        }
        Ok(QrLoginSession {
            link,
            token,
            expires_at: value.get("expires_at").and_then(Value::as_i64).unwrap_or(0),
        })
    }

    /// Poll a QR login session. Cookies are imported into the client on success.
    pub async fn qr_login_poll(&self, token: &str) -> Result<QrScanStatus> {
        let url =
            format!("https://www.zhihu.com/api/v3/account/api/login/qrcode/{token}/scan_info");
        let (http_status, value) = self
            .login_request(Method::GET, &url, None, SIGNIN_REFERER, true)
            .await?;
        let login_status = value
            .get("login_status")
            .and_then(Value::as_str)
            .or_else(|| value.get("loginStatus").and_then(Value::as_str))
            .unwrap_or_default()
            .to_uppercase();
        let success = value.get("user_id").is_some()
            || value
                .get("access_token")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
            || value
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            || value
                .get("logged_in")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            || matches!(
                login_status.as_str(),
                "CONFIRMED" | "LOGIN_SUCCESS" | "SUCCESS" | "OK" | "LOGGED_IN"
            );

        for key in ["cookie", "cookies"] {
            if let Some(raw) = value.get(key).and_then(Value::as_str) {
                for pair in raw.split(';') {
                    let Some((name, value)) = pair.trim().split_once('=') else {
                        continue;
                    };
                    let name = name.trim();
                    if name.is_empty()
                        || matches!(
                            name,
                            "Domain"
                                | "Path"
                                | "Expires"
                                | "Max-Age"
                                | "HttpOnly"
                                | "Secure"
                                | "SameSite"
                        )
                    {
                        continue;
                    }
                    self.set_cookie(name, value.trim());
                }
            }
        }
        if let Some(z_c0) = value.get("z_c0").and_then(Value::as_str) {
            if !z_c0.is_empty() {
                self.set_cookie("z_c0", z_c0);
            }
        }

        Ok(QrScanStatus {
            http_status,
            qr_status: value.get("status").and_then(Value::as_i64).unwrap_or(0),
            login_status,
            success,
            message: value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string),
            redirect: value
                .get("error")
                .and_then(|error| error.get("redirect"))
                .and_then(Value::as_str)
                .map(str::to_string),
            risk_control: http_status == 403
                || value
                    .get("error")
                    .and_then(|error| error.get("need_login"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
        })
    }

    /// Signed full search mapped to feed cards.
    pub async fn search(
        &self,
        query: &str,
        offset: u32,
        options: SearchOptions,
    ) -> Result<crate::feed::SearchPage> {
        let value = self.search_json(query, offset, options).await?;
        Ok(crate::feed::parse_search_page(&value))
    }

    /// Public search suggestions.
    pub async fn search_suggest(&self, query: &str) -> Result<Vec<String>> {
        let mut url = Url::parse("https://www.zhihu.com/api/v4/search/suggest")
            .map_err(|error| ApiError::Other(error.to_string()))?;
        url.query_pairs_mut().append_pair("q", query);
        let value = self.get_json(url.as_str(), false).await?;
        Ok(value
            .get("suggest")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("query").and_then(Value::as_str))
            .map(str::to_string)
            .collect())
    }

    pub async fn hot_search_json(&self) -> Result<Value> {
        self.get_json(
            "https://www.zhihu.com/api/v4/search/hot_search?limit=20",
            false,
        )
        .await
    }

    /// Public hot-search list (works without login).
    pub async fn hot_search(&self) -> Result<Vec<HotSearchItem>> {
        let value = self.hot_search_json().await?;
        let response: HotSearchResponse = serde_json::from_value(value)?;
        Ok(response
            .hot_search_queries
            .into_iter()
            .map(|item| HotSearchItem {
                id: item.query_id,
                query: item.query,
                hot_show: item.hot_show,
                label: item.label,
            })
            .collect())
    }

    pub async fn daily_latest_json(&self) -> Result<Value> {
        self.get_json("https://news-at.zhihu.com/api/4/stories/latest", false)
            .await
    }

    pub async fn daily_story(&self, id: u64) -> Result<zhihu_types::DailyDetail> {
        let url = format!("https://news-at.zhihu.com/api/4/news/{id}");
        let value = self.get_json(&url, false).await?;
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("知乎日报")
            .to_string();
        let body_html = value
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let hint = value
            .get("hint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let image = value
            .get("image")
            .and_then(Value::as_str)
            .or_else(|| {
                value
                    .get("images")
                    .and_then(Value::as_array)
                    .and_then(|images| images.first())
                    .and_then(Value::as_str)
            })
            .map(str::to_string);
        Ok(zhihu_types::DailyDetail {
            id,
            title,
            body_html,
            hint,
            image,
        })
    }

    /// Public Zhihu Daily stories (works without login).
    pub async fn daily_latest(&self) -> Result<Vec<DailyStory>> {
        let value = self.daily_latest_json().await?;
        let response: DailyResponse = serde_json::from_value(value)?;
        Ok(response
            .stories
            .into_iter()
            .map(|story| DailyStory {
                id: story.id,
                title: story.title,
                hint: story.hint,
                url: story.url,
                image: story.images.into_iter().next(),
            })
            .collect())
    }

    pub async fn me(&self) -> Result<Value> {
        self.get_json("https://www.zhihu.com/api/v4/me", true).await
    }

    /// Validate the session and return the current account profile.
    pub async fn me_profile(&self) -> Result<zhihu_types::AccountProfile> {
        let value = self.me().await?;
        let id = value
            .get("id")
            .and_then(|id| {
                id.as_str()
                    .map(str::to_string)
                    .or_else(|| id.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default();
        if id.is_empty() {
            return Err(ApiError::Other("account id is missing".into()));
        }
        Ok(zhihu_types::AccountProfile {
            id,
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("知乎用户")
                .to_string(),
            url_token: value
                .get("url_token")
                .and_then(Value::as_str)
                .map(str::to_string),
            avatar_url: value
                .get("avatar_url")
                .and_then(Value::as_str)
                .map(str::to_string),
            headline: value
                .get("headline")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// Answer detail. Requires login cookies.
    pub async fn answer_detail(&self, id: &str) -> Result<zhihu_types::ContentDetail> {
        let url = format!(
            "https://www.zhihu.com/api/v4/answers/{id}?include=content,excerpt,author,question,comment_count,voteup_count,created_time,updated_time"
        );
        let value = self.get_json(&url, true).await?;
        crate::detail::parse_answer(&value)
            .ok_or_else(|| ApiError::Other("answer detail is missing fields".into()))
    }

    /// Notification timeline (requires login).
    pub async fn notifications(
        &self,
        offset: u32,
    ) -> Result<crate::notifications::NotificationsPage> {
        let url =
            format!("https://api.zhihu.com/notifications/v3/message/v3?limit=20&offset={offset}");
        let value = self.get_json(&url, true).await?;
        Ok(crate::notifications::parse_notifications_page(&value))
    }

    /// Root comments for an answer/article (first page).
    pub async fn comments(
        &self,
        kind: zhihu_types::ContentKind,
        id: &str,
        offset: u32,
    ) -> Result<zhihu_types::CommentsPage> {
        let resource = match kind {
            zhihu_types::ContentKind::Answer => "answers",
            zhihu_types::ContentKind::Article => "articles",
            _ => {
                return Err(ApiError::Other(
                    "comments are only supported for answers/articles".into(),
                ))
            }
        };
        let url = format!(
            "https://www.zhihu.com/api/v4/{resource}/{id}/comments?limit=20&offset={offset}&include=content,author,reply_to_author,like_count,child_comment_count,created_time,is_author"
        );
        let referer = match kind {
            zhihu_types::ContentKind::Answer => format!("https://www.zhihu.com/answer/{id}"),
            zhihu_types::ContentKind::Article => format!("https://zhuanlan.zhihu.com/p/{id}"),
            _ => "https://www.zhihu.com/".to_string(),
        };
        let value = self.get_json_with_referer(&url, true, &referer).await?;
        Ok(crate::comments::parse_comments_v4_page(&value))
    }

    /// Mark every notification category as read (server-side).
    pub async fn mark_all_notifications_read(&self) -> Result<()> {
        let entries = ["comment", "like", "favlist_me", "follow", "invite"];
        let mut last_error = None;
        for entry in entries {
            let url = format!(
                "https://api.zhihu.com/notifications/v3/timeline/entry/{entry}/actions/readall"
            );
            if let Err(error) = self.send_json(Method::POST, &url, None, true).await {
                last_error = Some(error);
            }
        }
        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Child comments (replies) of one root comment.
    pub async fn child_comments(
        &self,
        comment_id: &str,
        offset: u32,
    ) -> Result<zhihu_types::CommentsPage> {
        let url = format!(
            "https://www.zhihu.com/api/v4/comments/{comment_id}/child_comments?limit=20&offset={offset}&include=content,author,reply_to_author,like_count,child_comment_count,created_time,is_author"
        );
        let value = self
            .get_json_with_referer(&url, true, "https://www.zhihu.com/")
            .await?;
        Ok(crate::comments::parse_comments_v4_page(&value))
    }

    /// Vote an answer up/down/neutral. Returns the new vote-up count.
    pub async fn vote_answer(&self, id: &str, state: &str) -> Result<u64> {
        let url = format!("https://www.zhihu.com/api/v4/answers/{id}/voters");
        let body = serde_json::json!({ "type": state });
        let value = self
            .send_json(Method::POST, &url, Some(&body), true)
            .await?;
        Ok(value
            .get("voteup_count")
            .and_then(Value::as_u64)
            .unwrap_or(0))
    }

    /// Vote an article up (`up`), down (`down`) or cancel (`neutral`).
    pub async fn vote_article(&self, id: &str, state: &str) -> Result<u64> {
        let url = format!("https://www.zhihu.com/api/v4/articles/{id}/voters");
        let voting = match state {
            "up" => 1,
            "down" => -1,
            _ => 0,
        };
        let body = serde_json::json!({ "voting": voting });
        let value = self
            .send_json(Method::POST, &url, Some(&body), true)
            .await?;
        Ok(value
            .get("voteup_count")
            .and_then(Value::as_u64)
            .unwrap_or(0))
    }

    pub async fn like_comment(&self, id: &str, liked: bool) -> Result<()> {
        let url = format!("https://www.zhihu.com/api/v4/comments/{id}/like");
        let body = serde_json::json!({ "like": liked });
        self.send_json(Method::POST, &url, Some(&body), true)
            .await?;
        Ok(())
    }

    /// Follow or unfollow a member.
    pub async fn follow_member(&self, url_token: &str, follow: bool) -> Result<Value> {
        let url = format!("https://www.zhihu.com/api/v4/members/{url_token}/followers");
        let method = if follow { Method::POST } else { Method::DELETE };
        self.send_json(method, &url, None, true).await
    }

    /// Article detail. Requires login cookies.
    pub async fn article_detail(&self, id: &str) -> Result<zhihu_types::ContentDetail> {
        let url = format!(
            "https://www.zhihu.com/api/v4/articles/{id}?include=content,excerpt,author,comment_count,voteup_count,created,updated"
        );
        let value = self.get_json(&url, true).await?;
        crate::detail::parse_article(&value)
            .ok_or_else(|| ApiError::Other("article detail is missing fields".into()))
    }

    pub async fn question_detail(&self, id: &str) -> Result<zhihu_types::QuestionDetail> {
        let url = format!(
            "https://www.zhihu.com/api/v4/questions/{id}?include=detail,answer_count,follower_count"
        );
        let value = self.get_json(&url, true).await?;
        crate::detail::parse_question(&value)
            .ok_or_else(|| ApiError::Other("question detail is missing fields".into()))
    }

    /// One page of answers for a question. `order` is `default`, `newest` or
    /// `liked`.
    pub async fn question_answers(
        &self,
        id: &str,
        offset: u32,
        order: &str,
    ) -> Result<(Vec<zhihu_types::FeedItem>, bool)> {
        let order_by = match order {
            "newest" => "updated",
            "liked" => "voteups",
            _ => "default",
        };
        let url = format!(
            "https://www.zhihu.com/api/v4/questions/{id}/answers?include=content,excerpt,author,question,comment_count,voteup_count,created_time,updated_time&limit=20&offset={offset}&order_by={order_by}"
        );
        let value = self.get_json(&url, true).await?;
        Ok(crate::feed::parse_answers_page(&value))
    }

    pub async fn member(&self, token: &str) -> Result<zhihu_types::MemberProfile> {
        let url = format!(
            "https://www.zhihu.com/api/v4/members/{token}?include=avatar_url,headline,description,follower_count,following_count,answer_count,articles_count,is_following"
        );
        let value = self.get_json(&url, true).await?;
        crate::detail::parse_member(&value)
            .ok_or_else(|| ApiError::Other("member detail is missing fields".into()))
    }

    pub async fn member_answers(
        &self,
        token: &str,
        offset: u32,
    ) -> Result<(Vec<zhihu_types::FeedItem>, bool)> {
        let url = format!(
            "https://www.zhihu.com/api/v4/members/{token}/answers?include=content,excerpt,author,question,comment_count,voteup_count,created_time,updated_time&limit=20&offset={offset}&order_by=created"
        );
        let value = self.get_json(&url, true).await?;
        Ok(crate::feed::parse_answers_page(&value))
    }

    /// Search questions only; used to jump from a hot query to a question page.
    pub async fn search_questions(&self, query: &str) -> Result<Vec<zhihu_types::FeedItem>> {
        let value = self
            .search_json(
                query,
                0,
                SearchOptions {
                    vertical: SearchVertical::Question,
                    sort: SearchSort::Default,
                },
            )
            .await?;
        Ok(crate::feed::parse_search_page(&value)
            .items
            .into_iter()
            .filter(|item| item.kind == zhihu_types::ContentKind::Question)
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct HotSearchResponse {
    #[serde(default)]
    hot_search_queries: Vec<HotQuery>,
}

#[derive(Debug, Deserialize)]
struct HotQuery {
    #[serde(default)]
    query_id: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    hot_show: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DailyResponse {
    #[serde(default)]
    stories: Vec<DailyStoryDto>,
}

#[derive(Debug, Deserialize)]
struct DailyStoryDto {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    hint: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    images: Vec<String>,
}

/// Zhihu's web client uses a device cookie shaped like
/// `A<opaque>|<unix_millis>`. The exact opaque part is not otherwise used.
fn synthetic_dc0() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut opaque = String::with_capacity(22);
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    for chunk in bytes.chunks(3) {
        let mut v = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            v |= (*byte as u32) << (8 * i);
        }
        for i in 0..4 {
            opaque.push(ALPHABET[((v >> (6 * i)) & 63) as usize] as char);
        }
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("A{opaque}|{millis}")
}
