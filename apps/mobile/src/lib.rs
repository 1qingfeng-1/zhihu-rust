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

    #[no_mangle]
    fn android_main(app: android_activity::AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("zhihu-rust"),
        );
        zhihu_ui::install_panic_hook();
        jni::install_panic_hook();

        let _platform = jni::init_platform(&app);
        let Some(shared) = jni::shared_platform() else {
            log::error!("shared_platform() returned None");
            return;
        };

        Application::with_platform(shared.into_rc())
            .with_assets(gpui_kit::assets::Assets::new(""))
            .run(|cx: &mut App| {
                log::info!("opening main window");
                open_main_window(cx);
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
