//! Zhihu Android phone-number login (SMS code).
//!
//! This mirrors the official Android client protocol: a guest token is
//! initialized with an encrypted device form, the SMS code is requested via
//! `/api/account/prod/auth/digits`, and `/api/account/prod/sign_in` exchanges
//! it for an access token plus cookies. Captcha is a server-side branch and is
//! surfaced to the caller instead of being treated as an error.
//!
//! Ported from Zhihu++ (`ZhihuPhoneLoginClient.kt`, AGPL-3.0).

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::client::{ApiError, Result};
use crate::phone_crypto::{encrypt_body, hmac_sha1_hex};

const API_BASE: &str = "https://api.zhihu.com";
const UDID_URL: &str = "https://www.zhihu.com/udid";
const DEVICE_GUEST_INIT_PATH: &str = "/api/account/prod/init/udid_guest";
const CAPTCHA_PATH: &str = "/captcha";
const AUTH_DIGITS_PATH: &str = "/api/account/prod/auth/digits";
const SIGN_IN_PATH: &str = "/api/account/prod/sign_in";

const MOBILE_CLIENT_ID: &str = "8d5227e0aaaa4797a763ac64e0c3b8";
const MOBILE_CLIENT_SECRET: &str = "ecbefbf6b17e47ecb9035107866380";
const MOBILE_SOURCE: &str = "com.zhihu.android";
const DIGITS_GRANT_TYPE: &str = "digits";
const CLOUD_APP_ID: &str = "1355";
const CLOUD_APP_SECRET: &str = "dd49a835-56e7-4a0f-95b5-efd51ea5397f";
const CLOUD_SIGN_VERSION: &str = "2";

const MOBILE_USER_AGENT: &str = "com.zhihu.android/Futureve/11.4.0 Mozilla/5.0 \
(Linux; Android 12; Android SDK built for arm64 Build/SE1A.220621.001; wv) \
AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/57.0.1000.10 \
Mobile Safari/537.36";
const APP_ZA: &str = "OS=Android&Release=12&Model=Android+SDK+built+for+arm64&VersionName=11.4.0&\
VersionCode=40408&Product=com.zhihu.android&Width=1080&Height=2274&\
Installer=%E8%8D%A3%E8%80%80%E5%95%86%E5%BA%97&DeviceType=AndroidPhone&Brand=Android";

const CAPTCHA_INVALID_ERROR_CODES: [i64; 2] = [120001, 120002];
const CAPTCHA_NEEDED_ERROR_CODE: i64 = 120005;

/// Device fields included in the guest-initialization form.
struct DeviceInfo {
    timezone_offset_seconds: i64,
    app_install_time_millis: i64,
    notification_enabled: bool,
    bluetooth_available: bool,
    phone_brand: &'static str,
    phone_model: &'static str,
    android_release: &'static str,
    cpu_type: &'static str,
    cpu_count: u32,
    cpu_usage: &'static str,
    total_memory_megabytes: u32,
    free_memory_megabytes: u32,
    total_storage_megabytes: u32,
    free_storage_megabytes: u32,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            timezone_offset_seconds: 8 * 3600,
            app_install_time_millis: 1_700_000_000_000,
            notification_enabled: false,
            bluetooth_available: true,
            phone_brand: "Android",
            phone_model: "Android SDK built for arm64",
            android_release: "12",
            cpu_type: "aarch64",
            cpu_count: 4,
            cpu_usage: "0.0",
            total_memory_megabytes: 256,
            free_memory_megabytes: 128,
            total_storage_megabytes: 65_536,
            free_storage_megabytes: 32_768,
        }
    }
}

/// Result of asking the server to send an SMS code.
#[derive(Clone, Debug, PartialEq)]
pub enum DigitsOutcome {
    Sent,
    CaptchaRequired { image_base64: Option<String> },
}

/// Login credentials returned by `/api/account/prod/sign_in`.
#[derive(Clone, Debug)]
pub struct PhoneLoginToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: Option<i64>,
    pub cookies: HashMap<String, String>,
}

/// Stateful phone-login session. Keep one instance per login attempt: the
/// guest token, device id and cookies carry across send-code and sign-in.
pub struct PhoneLoginSession {
    http: Client,
    cookies: HashMap<String, String>,
    authorization: String,
    device_id: Option<String>,
    web_device_cookie: Option<String>,
    device: DeviceInfo,
}

impl PhoneLoginSession {
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(MOBILE_USER_AGENT)
            .build()
            .map_err(ApiError::Http)?;
        Ok(Self {
            http,
            cookies: HashMap::new(),
            authorization: format!("oauth {MOBILE_CLIENT_ID}"),
            device_id: None,
            web_device_cookie: None,
            device: DeviceInfo::default(),
        })
    }

    /// Validate and normalize a mainland China mobile number to `+86...`.
    pub fn normalize_phone(phone: &str) -> Result<String> {
        let compact: String = phone
            .trim()
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '-')
            .collect();
        let local = if let Some(rest) = compact.strip_prefix("+86") {
            rest
        } else if compact.len() == 13 && compact.starts_with("86") {
            &compact[2..]
        } else {
            compact.as_str()
        };
        if local.len() != 11
            || !local.starts_with('1')
            || !local.chars().all(|ch| ch.is_ascii_digit())
        {
            return Err(ApiError::Other("请输入正确的中国大陆手机号".to_string()));
        }
        Ok(format!("+86{local}"))
    }

    /// Warm up the guest token and device identity without sending an SMS.
    pub async fn initialize(&mut self) -> Result<()> {
        self.ensure_guest().await
    }

    /// Whether the server currently asks for an image captcha.
    pub async fn captcha_available(&mut self) -> Result<bool> {
        Ok(self.request_captcha_state().await?.show_captcha)
    }

    pub async fn request_digits(&mut self, phone: &str) -> Result<DigitsOutcome> {
        let username = Self::normalize_phone(phone)?;
        self.ensure_guest().await?;

        let state = self.request_captcha_state().await?;
        if state.show_captcha {
            let image = self.refresh_captcha().await?;
            return Ok(DigitsOutcome::CaptchaRequired {
                image_base64: image,
            });
        }
        self.send_digits(&username, true).await
    }

    async fn send_digits(&mut self, username: &str, recheck: bool) -> Result<DigitsOutcome> {
        let form = pair_form(&[("username", username), ("client_id", MOBILE_CLIENT_ID)]);
        let response = self
            .apply_headers(self.http.post(format!("{API_BASE}{AUTH_DIGITS_PATH}")))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(encrypt_body(&form))
            .send()
            .await?;
        self.absorb_cookies(&response);

        let status = response.status();
        let body = response.text().await?;
        if status.is_success() {
            return Ok(DigitsOutcome::Sent);
        }

        let error = error_object(&body);
        let code = error
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(value_as_i64);
        if code.is_some_and(|code| CAPTCHA_INVALID_ERROR_CODES.contains(&code)) {
            let image = self.refresh_captcha().await?;
            return Ok(DigitsOutcome::CaptchaRequired {
                image_base64: image,
            });
        }
        if code == Some(CAPTCHA_NEEDED_ERROR_CODE) && recheck {
            let state = self.request_captcha_state().await?;
            if state.show_captcha {
                let image = self.refresh_captcha().await?;
                return Ok(DigitsOutcome::CaptchaRequired {
                    image_base64: image,
                });
            }
            return Box::pin(self.send_digits(username, false)).await;
        }

        Err(api_error("发送短信验证码失败", status.as_u16(), &body))
    }

    pub async fn refresh_captcha(&mut self) -> Result<Option<String>> {
        self.ensure_guest().await?;
        let response = self
            .apply_headers(self.http.put(format!("{API_BASE}{CAPTCHA_PATH}")))
            .send()
            .await?;
        self.absorb_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(api_error("获取图形验证码失败", status.as_u16(), &body));
        }
        Ok(serde_json::from_str::<CaptchaResponse>(&body)
            .map_err(ApiError::Json)?
            .img_base64)
    }

    pub async fn verify_captcha(&mut self, input: &str) -> Result<bool> {
        if input.trim().is_empty() {
            return Err(ApiError::Other("图形验证码不能为空".to_string()));
        }
        self.ensure_guest().await?;
        let form = pair_form(&[("input_text", input.trim())]);
        let response = self
            .apply_headers(self.http.post(format!("{API_BASE}{CAPTCHA_PATH}")))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(encrypt_body(&form))
            .send()
            .await?;
        self.absorb_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(api_error("验证图形验证码失败", status.as_u16(), &body));
        }
        Ok(serde_json::from_str::<CaptchaVerificationResponse>(&body)
            .map_err(ApiError::Json)?
            .success)
    }

    pub async fn sign_in(&mut self, phone: &str, digits: &str) -> Result<PhoneLoginToken> {
        let username = Self::normalize_phone(phone)?;
        if digits.trim().is_empty() {
            return Err(ApiError::Other("短信验证码不能为空".to_string()));
        }
        self.ensure_guest().await?;

        let timestamp = epoch_seconds().to_string();
        let signature = hmac_sha1_hex(
            MOBILE_CLIENT_SECRET,
            &format!("{DIGITS_GRANT_TYPE}{MOBILE_CLIENT_ID}{MOBILE_SOURCE}{timestamp}"),
        );
        let form = pair_form(&[
            ("client_id", MOBILE_CLIENT_ID),
            ("digits", digits.trim()),
            ("grant_type", DIGITS_GRANT_TYPE),
            ("signature", &signature),
            ("source", MOBILE_SOURCE),
            ("timestamp", &timestamp),
            ("username", &username),
        ]);
        let response = self
            .apply_headers(self.http.post(format!("{API_BASE}{SIGN_IN_PATH}")))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(encrypt_body(&form))
            .send()
            .await?;
        self.absorb_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(api_error("手机号登录失败", status.as_u16(), &body));
        }

        let token: TokenResponse = serde_json::from_str(&body).map_err(ApiError::Json)?;
        if token.access_token.is_empty() {
            return Err(ApiError::Other("服务器未返回登录凭证".to_string()));
        }
        self.cookies.extend(token.cookie);
        if !self.cookies.contains_key("d_c0") {
            let device_cookie = self
                .web_device_cookie
                .clone()
                .ok_or_else(|| ApiError::Other("服务器未返回必要的 Cookie d_c0".into()))?;
            self.cookies.insert("d_c0".to_string(), device_cookie);
        }

        Ok(PhoneLoginToken {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            token_type: token.token_type,
            expires_in: token.expires_in,
            cookies: self.cookies.clone(),
        })
    }

    async fn ensure_guest(&mut self) -> Result<()> {
        if !self.authorization.starts_with("oauth ") {
            return Ok(());
        }

        if self.web_device_cookie.is_none() {
            let response = self.http.post(UDID_URL).send().await?;
            self.web_device_cookie = set_cookie(&response, "d_c0");
            if !response.status().is_success() {
                return Err(ApiError::Other(format!(
                    "初始化网页设备凭证失败（HTTP {}）",
                    response.status().as_u16()
                )));
            }
        }

        let timestamp = epoch_seconds().to_string();
        let form = pair_form(&[
            ("app_build", "40408"),
            (
                "app_install_time",
                &self.device.app_install_time_millis.to_string(),
            ),
            ("app_ticket", "interface is empty"),
            ("app_version", "11.4.0"),
            (
                "bt_ck",
                if self.device.bluetooth_available {
                    "1"
                } else {
                    "0"
                },
            ),
            ("bundle_id", MOBILE_SOURCE),
            ("cp_ct", &self.device.cpu_count.to_string()),
            ("cp_tp", self.device.cpu_type),
            ("cp_us", self.device.cpu_usage),
            ("d_n", self.device.phone_model),
            ("fr_mem", &self.device.free_memory_megabytes.to_string()),
            ("fr_st", &self.device.free_storage_megabytes.to_string()),
            ("latitude", "0.0"),
            ("longitude", "0.0"),
            (
                "nt_st",
                if self.device.notification_enabled {
                    "1"
                } else {
                    "0"
                },
            ),
            ("ph_br", self.device.phone_brand),
            ("ph_md", self.device.phone_model),
            ("ph_os", &format!("Android {}", self.device.android_release)),
            ("pre_install", "InterfaceIsNull"),
            ("tt_mem", &self.device.total_memory_megabytes.to_string()),
            ("tt_st", &self.device.total_storage_megabytes.to_string()),
            ("tz_of", &self.device.timezone_offset_seconds.to_string()),
            ("zx_expired", "0"),
        ]);
        let signature = hmac_sha1_hex(
            CLOUD_APP_SECRET,
            &format!("{CLOUD_APP_ID}{CLOUD_SIGN_VERSION}{form}{timestamp}"),
        );

        let response = self
            .apply_headers(
                self.http
                    .post(format!("{API_BASE}{DEVICE_GUEST_INIT_PATH}")),
            )
            .header("x-app-id", CLOUD_APP_ID)
            .header("x-sign-version", CLOUD_SIGN_VERSION)
            .header("x-req-ts", timestamp)
            .header("x-req-signature", signature)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(encrypt_body(&form))
            .send()
            .await?;
        self.absorb_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(api_error("初始化手机号登录失败", status.as_u16(), &body));
        }

        let init: GuestInitResponse = serde_json::from_str(&body).map_err(ApiError::Json)?;
        if init.guest.access_token.is_empty() || init.udid.is_empty() {
            return Err(ApiError::Other("服务器未返回访客凭证".to_string()));
        }
        self.cookies.extend(init.guest.cookie);
        let token_type = if init.guest.token_type.is_empty() {
            "bearer".to_string()
        } else {
            init.guest.token_type
        };
        self.authorization = format!("{token_type} {}", init.guest.access_token);
        self.device_id = Some(init.udid);
        Ok(())
    }

    async fn request_captcha_state(&mut self) -> Result<CaptchaResponse> {
        let response = self
            .apply_headers(self.http.get(format!("{API_BASE}{CAPTCHA_PATH}")))
            .send()
            .await?;
        self.absorb_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(api_error("检查图形验证码失败", status.as_u16(), &body));
        }
        serde_json::from_str(&body).map_err(ApiError::Json)
    }

    fn absorb_cookies(&mut self, response: &reqwest::Response) {
        for value in response.headers().get_all(SET_COOKIE) {
            let Ok(line) = value.to_str() else {
                continue;
            };
            let pair = line.split(';').next().unwrap_or_default().trim();
            if let Some((name, value)) = pair.split_once('=') {
                if !name.is_empty() && !value.is_empty() {
                    self.cookies.insert(name.to_string(), value.to_string());
                }
            }
        }
    }

    fn apply_headers(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut request = request
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, MOBILE_USER_AGENT)
            .header(AUTHORIZATION, &self.authorization)
            .header("x-api-version", "3.0.93")
            .header("x-app-version", "11.4.0")
            .header("x-app-build", "release")
            .header("x-app-bundleid", MOBILE_SOURCE)
            .header("x-app-flavor", "honor")
            .header("x-app-za", APP_ZA)
            .header("x-network-type", "3G")
            .header("x-page-id", "44")
            .header("x-zse-93", "101_1_1.0");
        let cookie = cookie_header(&self.cookies);
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie);
        }
        if let Some(device_id) = &self.device_id {
            request = request.header("x-udid", device_id);
        }
        request
    }
}

fn pair_form(entries: &[(&str, &str)]) -> String {
    let mut serializer = Serializer::new(String::new());
    for (name, value) in entries {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

fn cookie_header(cookies: &HashMap<String, String>) -> String {
    let mut entries: Vec<String> = cookies
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    entries.sort();
    entries.join("; ")
}

fn set_cookie(response: &reqwest::Response, name: &str) -> Option<String> {
    for value in response.headers().get_all(SET_COOKIE) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for part in value.split('\u{1f}').flat_map(|part| part.split(", ")) {
            let pair = part.split(';').next().unwrap_or_default().trim();
            if let Some((key, val)) = pair.split_once('=') {
                if key == name && !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn error_object(body: &str) -> Option<Value> {
    let root: Value = serde_json::from_str(body).ok()?;
    Some(root.get("error").cloned().unwrap_or(root))
}

fn api_error(prefix: &str, status: u16, body: &str) -> ApiError {
    let object = error_object(body);
    let message = object
        .as_ref()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{prefix}（HTTP {status}）"));
    match object
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(value_as_i64)
    {
        Some(code) => ApiError::Other(format!("{message}（code {code}）")),
        None => ApiError::Other(message),
    }
}

#[derive(Debug, Deserialize)]
struct GuestTokenResponse {
    #[serde(default, alias = "accessToken")]
    access_token: String,
    #[serde(default, alias = "tokenType")]
    token_type: String,
    #[serde(default)]
    cookie: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct GuestInitResponse {
    #[serde(default)]
    udid: String,
    guest: GuestTokenResponse,
}

#[derive(Debug, Default, Deserialize)]
struct CaptchaResponse {
    #[serde(default, alias = "showCaptcha")]
    show_captcha: bool,
    #[serde(default, alias = "imgBase64")]
    img_base64: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CaptchaVerificationResponse {
    #[serde(default)]
    success: bool,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default, alias = "accessToken")]
    access_token: String,
    #[serde(default, alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(default = "default_token_type", alias = "tokenType")]
    token_type: String,
    #[serde(default, alias = "expiresIn")]
    expires_in: Option<i64>,
    #[serde(default)]
    cookie: HashMap<String, String>,
}

fn default_token_type() -> String {
    "bearer".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_mainland_phone_numbers() {
        assert_eq!(
            PhoneLoginSession::normalize_phone("138 0013 8000").unwrap(),
            "+8613800138000"
        );
        assert_eq!(
            PhoneLoginSession::normalize_phone("+86 138-0013-8000").unwrap(),
            "+8613800138000"
        );
        assert_eq!(
            PhoneLoginSession::normalize_phone("8613800138000").unwrap(),
            "+8613800138000"
        );
        assert!(PhoneLoginSession::normalize_phone("12345").is_err());
        assert!(PhoneLoginSession::normalize_phone("23800138000").is_err());
        assert!(PhoneLoginSession::normalize_phone("1380013800a").is_err());
    }

    #[test]
    fn error_body_extracts_nested_message() {
        let body = r#"{"error":{"code":120005,"message":"需要验证"}}"#;
        let error = api_error("失败", 400, body);
        assert_eq!(error.to_string(), "需要验证（code 120005）");
        assert_eq!(
            error_object(body)
                .and_then(|value| value.get("code").cloned())
                .and_then(|value| value_as_i64(&value)),
            Some(120005)
        );
    }

    #[test]
    fn cookie_header_is_sorted_and_skips_empty_values() {
        let mut cookies = HashMap::new();
        cookies.insert("z_c0".to_string(), "z".to_string());
        cookies.insert("d_c0".to_string(), "d".to_string());
        cookies.insert("empty".to_string(), String::new());
        assert_eq!(cookie_header(&cookies), "d_c0=d; z_c0=z");
    }
}
