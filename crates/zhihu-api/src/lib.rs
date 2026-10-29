//! Zhihu HTTP API: authentication, request signing and typed endpoints.
//!
//! The signing algorithm and protocol constants are ported from Zhihu++
//! (AGPL-3.0); see `sign.rs` and `docs/REWRITE_PLAN.md`.

pub mod client;
pub mod comments;
pub mod detail;
pub mod feed;
pub mod notifications;
pub mod phone_crypto;
pub mod phone_login;
pub mod sign;

pub use client::{
    ApiError, QrLoginSession, QrScanStatus, Result, SearchOptions, SearchSort, SearchVertical,
    ZhihuClient, DEFAULT_USER_AGENT,
};
pub use phone_login::{DigitsOutcome, PhoneLoginSession, PhoneLoginToken};
pub use sign::{encrypt_zse_v4, md5_hex, pathname, zse96, ZHIHU_WEB_ZSE93};

pub const API_HOST: &str = "https://www.zhihu.com";
