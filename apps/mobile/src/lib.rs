//! Mobile entry points for Android and iOS.
//!
//! Android is hosted by `NativeActivity` (or `GpuiInputActivity` once the
//! Gradle shell lands); iOS uses the static library + UIKit host.

use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{App, WindowOptions};
use zhihu_ui::ZhihuApp;

/// Open the app window once the platform surface exists.
pub fn open_main_window(cx: &mut App) {
    gpui_kit::init(cx);

    let client = gpui_pre_reqwest::Client::builder()
        .build()
        .expect("failed to create image HTTP client");
    let http_client: reqwest_client::ReqwestClient = client.into();
    cx.set_http_client(Arc::new(http_client));

    cx.open_window(WindowOptions::default(), |window, cx| {
        let view = cx.new(|cx| ZhihuApp::new(window, cx));
        cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
    })
    .expect("failed to open main window");
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use gpui_kit::Application;
    use gpui_mobile::android::jni;

    /// Install the Android logger.
    ///
    /// Release builds wrap it to drop `android-activity`'s per-event
    /// "Spurious ALOOPER_POLL_CALLBACK" error: the idle GPUI loop can hit it
    /// several times per second, and every line is a synchronous write to
    /// logd that keeps the CPU awake. Debug builds keep the message.
    fn init_android_logging() {
        let config = android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("zhihu-rust");

        #[cfg(not(debug_assertions))]
        {
            struct ReleaseLogger(android_logger::AndroidLogger);

            impl log::Log for ReleaseLogger {
                fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                    self.0.enabled(metadata)
                }

                fn log(&self, record: &log::Record<'_>) {
                    if record.level() == log::Level::Error
                        && record.target().starts_with("android_activity")
                        && record
                            .args()
                            .to_string()
                            .contains("Spurious ALOOPER_POLL_CALLBACK")
                    {
                        return;
                    }
                    self.0.log(record);
                }

                fn flush(&self) {
                    self.0.flush();
                }
            }

            static LOGGER: std::sync::OnceLock<ReleaseLogger> = std::sync::OnceLock::new();
            let logger =
                LOGGER.get_or_init(|| ReleaseLogger(android_logger::AndroidLogger::new(config)));
            if log::set_logger(logger).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
            }
        }

        #[cfg(debug_assertions)]
        android_logger::init_once(config);
    }

    #[no_mangle]
    fn android_main(app: android_activity::AndroidApp) {
        init_android_logging();
        zhihu_ui::install_panic_hook();
        jni::install_panic_hook();

        let _platform = jni::init_platform(&app);
        let Some(shared) = jni::shared_platform() else {
            log::error!("shared_platform() returned None");
            return;
        };

        Application::with_platform(shared.into_rc())
            .with_assets(gpui_kit::assets::AllAssets::new(""))
            .run(|cx: &mut App| {
                log::info!("opening main window");
                open_main_window(cx);
            });
    }

    /// Forward the Android back gesture into the GPUI event loop.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_dev_gpui_mobile_GpuiActivity_nativeBack(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
    ) {
        gpui_mobile::android::host::key(4, 0, 0);
        gpui_mobile::android::host::key(4, 1, 0);
    }

    /// Whether the Rust router has a route or overlay to dismiss.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_dev_gpui_mobile_GpuiActivity_nativeCanGoBack(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
    ) -> ::jni::sys::jboolean {
        zhihu_ui::android_nav::can_go_back()
    }

    /// Cookies read from the in-app sign-in WebView.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_dev_gpui_mobile_ZhihuLogin_nativeCookieHeader(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
        raw: *mut std::ffi::c_void,
    ) {
        let _ = jni::with_env(|env| {
            let obj = unsafe { ::jni::objects::JObject::from_raw(env, raw as ::jni::sys::jobject) };
            let raw = jni::get_string(env, &obj);
            if !raw.is_empty() {
                zhihu_ui::android_login::store_cookie_header(raw);
            }
            Ok(())
        });
    }

    /// Cookies read back from the risk-verification WebView.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_dev_gpui_mobile_ZhihuLogin_nativeSessionCookies(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
        raw: *mut std::ffi::c_void,
    ) {
        let _ = jni::with_env(|env| {
            let obj = unsafe { ::jni::objects::JObject::from_raw(env, raw as ::jni::sys::jobject) };
            let raw = jni::get_string(env, &obj);
            if !raw.is_empty() {
                zhihu_ui::android_login::store_session_cookies(raw);
            }
            Ok(())
        });
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use super::*;

    #[unsafe(no_mangle)]
    pub extern "C" fn gpui_ios_register_app() {
        gpui_mobile::ios::ffi::set_app_callback(Box::new(|cx: &mut App| {
            open_main_window(cx);
        }));
    }

    pub fn ios_main() {
        zhihu_ui::install_panic_hook();
        gpui_ios_register_app();
        gpui_mobile::ios::ffi::run_app();
    }
}
