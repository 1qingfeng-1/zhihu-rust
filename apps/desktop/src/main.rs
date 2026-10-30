use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{px, size, App, Bounds, WindowBounds, WindowOptions};
use zhihu_ui::ZhihuApp;

fn main() {
    zhihu_ui::install_panic_hook();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets::new(""))
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            cx.set_app_identity("dev.zhihurs.app", "知乎 Rust");

            let client = gpui_pre_reqwest::Client::builder()
                .build()
                .expect("failed to create image HTTP client");
            let http_client: reqwest_client::ReqwestClient = client.into();
            cx.set_http_client(Arc::new(http_client));

            let bounds = Bounds::centered(None, size(px(1180.), px(820.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.), px(520.))),
                    titlebar: Some(gpui_kit::TitlebarOptions {
                        title: Some("知乎 Rust".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| ZhihuApp::new(window, cx));
                    cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
                },
            )
            .expect("failed to open main window");

            cx.activate(true);
        });
}
