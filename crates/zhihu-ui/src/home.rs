//! Binance-inspired mobile home: dark-first palette, yellow accent, bottom
//! navigation and compact content cards.

use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, img, px, rgb, uniform_list, AnyElement, Context, Div, FontWeight, MouseButton, Rgba,
    SharedString, Window,
};
use zhihu_types::FeedItem;

use super::{
    daily_row, empty_state, feed_card, hot_row, open_external, safe_area, TypographyPreset,
    ZhihuApp, TABS, UI_FONT,
};

fn feed_card_height(item: &FeedItem) -> f32 {
    let title_lines = item.title.chars().count().div_ceil(18).clamp(1, 2);
    let excerpt_lines = item.excerpt.chars().count().div_ceil(24).clamp(1, 3);
    112.0 + title_lines as f32 * 24.0 + excerpt_lines as f32 * 20.0
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Rgba,
    surface: Rgba,
    elevated: Rgba,
    border: Rgba,
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    accent_text: Rgba,
    danger: Rgba,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: rgb(0x0b0e11),
            surface: rgb(0x181a20),
            elevated: rgb(0x1e2329),
            border: rgb(0x2b3139),
            fg: rgb(0xeaecef),
            muted: rgb(0x848e9c),
            accent: rgb(0xf0b90b),
            accent_text: rgb(0x0b0e11),
            danger: rgb(0xf6465d),
        }
    } else {
        Palette {
            bg: rgb(0xfafafa),
            surface: rgb(0xffffff),
            elevated: rgb(0xf5f5f5),
            border: rgb(0xeaecef),
            fg: rgb(0x1e2329),
            muted: rgb(0x707a8a),
            accent: rgb(0xf0b90b),
            accent_text: rgb(0x1e2329),
            danger: rgb(0xf6465d),
        }
    }
}

const NAV: [&str; 5] = ["推荐", "关注", "热榜", "日报", "我的"];
const SIDEBAR_NAV_IDS: [&str; 5] = [
    "side-nav-0",
    "side-nav-1",
    "side-nav-2",
    "side-nav-3",
    "side-nav-4",
];
const MINI_NAV_IDS: [&str; 5] = [
    "mini-nav-0",
    "mini-nav-1",
    "mini-nav-2",
    "mini-nav-3",
    "mini-nav-4",
];

/// Desktop wide-window breakpoint. Tablets keep the mobile layout because the
/// touch navigation and safe-area handling are tuned for it.
pub(crate) fn wide_layout(window: &Window) -> bool {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        window.viewport_size().width >= px(900.)
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = window;
        false
    }
}

impl ZhihuApp {
    pub(crate) fn render_home(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(self.dark);
        let wide = wide_layout(window);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let account_label = self.account_label();
        let logged_in = self.is_logged_in();
        let avatar = account_label.chars().next().unwrap_or('我').to_string();

        let header = div()
            .h(px(52.))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .bg(p.bg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(20.)).rounded_md().bg(p.accent))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.fg)
                            .child("知乎"),
                    )
                    .child(
                        div()
                            .px_1()
                            .py_0p5()
                            .rounded_sm()
                            .bg(p.elevated)
                            .text_xs()
                            .text_color(p.muted)
                            .child("Rust"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("home-notifications")
                            .text_sm()
                            .text_color(p.muted)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_notifications();
                                cx.notify();
                            }))
                            .child("消息"),
                    )
                    .child(
                        div()
                            .id("home-profile")
                            .size(px(30.))
                            .rounded_full()
                            .bg(if logged_in { p.accent } else { p.elevated })
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_tab(4);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if logged_in { p.accent_text } else { p.muted })
                                    .child(SharedString::from(avatar)),
                            ),
                    ),
            );

        let search = div()
            .px_4()
            .pb_2()
            .bg(p.bg)
            .flex()
            .items_center()
            .gap_2()
            .child(
                Input::new(&self.search)
                    .flex_1()
                    .h(px(40.))
                    .rounded_full()
                    .bg(p.elevated)
                    .text_color(p.fg),
            )
            .child(
                div()
                    .id("search-go")
                    .size(px(40.))
                    .rounded_full()
                    .bg(p.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let query = this.search.read(cx).value().trim().to_string();
                        if !query.is_empty() {
                            this.open_search(query);
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.accent_text)
                            .child("搜索"),
                    ),
            );

        let content: AnyElement = if self.blocklist_open {
            self.render_blocklist_panel(cx, &p)
        } else {
            match self.tab {
                0 | 1 => self.render_feed_list(cx, p, wide),
                2 => self.render_hot_list(cx, p),
                3 => self.render_daily_list(cx, p),
                _ => self.render_profile(cx, &p),
            }
        };

        let tab_error = match self.tab {
            0 | 1 => self.feed_error.as_ref(),
            2 => self.hot_error.as_ref(),
            3 => self.daily_error.as_ref(),
            _ => self.error.as_ref(),
        };
        let status_text = if let Some(error) = tab_error {
            error.clone()
        } else {
            match self.tab {
                0 | 1 => {
                    let count = if self.feed.is_empty() {
                        self.items.len()
                    } else {
                        self.feed.len()
                    };
                    format!("推荐 · {count} 条")
                }
                2 => format!("热榜 · {} 条", self.hot.len()),
                3 => format!("日报 · {} 条", self.daily.len()),
                _ => {
                    if logged_in {
                        format!("已登录：{account_label}")
                    } else {
                        "未登录 · 导入 Cookie 解锁推荐流".to_string()
                    }
                }
            }
        };

        let status = div()
            .h(px(22.))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .bg(p.bg)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(if tab_error.is_some() {
                        p.danger
                    } else {
                        p.muted
                    })
                    .child(SharedString::from(status_text)),
            )
            .child(
                div()
                    .id("home-blocklist")
                    .text_xs()
                    .text_color(p.accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.blocklist = this.services.block_keywords();
                        this.blocklist_open = true;
                        cx.notify();
                    }))
                    .child("屏蔽词"),
            );

        let nav = div()
            .h(px(56.))
            .bg(p.surface)
            .border_t_1()
            .border_color(p.border)
            .flex()
            .children(NAV.iter().enumerate().map(|(index, label)| {
                let active = self.tab == index;
                div()
                    .id(("bottom-nav", index))
                    .aria_label(SharedString::from(*label))
                    .aria_selected(active)
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_tab(index);
                        cx.notify();
                    }))
                    .child(div().h(px(2.)).w(px(18.)).rounded_full().bg(if active {
                        p.accent
                    } else {
                        p.surface
                    }))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(if active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if active { p.accent } else { p.muted })
                            .child(*label),
                    )
            }));

        if wide {
            let sidebar = self.render_sidebar(cx, p);
            let title = TABS[self.tab.min(TABS.len() - 1)];
            let page_header = div()
                .h(px(56.))
                .px_6()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(p.border)
                .bg(p.bg)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_3()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(p.fg)
                                .child(SharedString::from(title)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted)
                                .child("知乎 · Rust 客户端"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("wide-refresh")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(p.elevated)
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_current(cx);
                                    cx.notify();
                                }))
                                .child("刷新 Ctrl+R"),
                        )
                        .child(
                            div()
                                .id("wide-theme")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(p.elevated)
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_theme();
                                    cx.notify();
                                }))
                                .child(if self.dark {
                                    "浅色 Ctrl+D"
                                } else {
                                    "深色 Ctrl+D"
                                }),
                        ),
                );

            return div()
                .size_full()
                .flex()
                .bg(p.bg)
                .text_color(p.fg)
                .font_family(UI_FONT)
                .pt(px(inset_top))
                .pb(px(inset_bottom))
                .pl(px(inset_left))
                .pr(px(inset_right))
                .track_focus(&self.focus_handle)
                .on_key_down(
                    cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                        if this.handle_shortcut(event, window, cx) {
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }),
                )
                .child(sidebar)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(page_header)
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(content)
                                .child(self.swipe_capture_layer(cx)),
                        )
                        .child(status),
                )
                .into_any_element();
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.handle_shortcut(event, window, cx) {
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .child(header)
            .child(search)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(content)
                    .child(self.swipe_capture_layer(cx)),
            )
            .child(status)
            .child(nav)
            .into_any_element()
    }

    /// Left navigation rail used when the desktop window is wide enough.
    fn render_sidebar(&mut self, cx: &mut Context<Self>, p: Palette) -> AnyElement {
        let account_label = self.account_label();
        let logged_in = self.is_logged_in();
        let avatar = account_label.chars().next().unwrap_or('我').to_string();
        let on_home = self.stack.is_empty();

        let mut nav = div().flex().flex_col().gap_1();
        for (index, label) in NAV.iter().enumerate() {
            let active = on_home && self.tab == index && !self.blocklist_open;
            nav = nav.child(self.sidebar_row(
                SIDEBAR_NAV_IDS[index],
                label,
                active,
                p,
                cx,
                move |this, cx| this.switch_tab(index, cx),
            ));
        }

        let mut actions = div().flex().flex_col().gap_1();
        actions = actions
            .child(self.sidebar_row(
                "side-notifications",
                "消息",
                matches!(self.stack.last(), Some(super::Route::Notifications)),
                p,
                cx,
                |this, _| {
                    this.go_root();
                    this.open_notifications();
                },
            ))
            .child(self.sidebar_row(
                "side-history",
                "阅读历史",
                matches!(self.stack.last(), Some(super::Route::History)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_history(cx);
                },
            ))
            .child(self.sidebar_row(
                "side-crashes",
                "崩溃日志",
                matches!(self.stack.last(), Some(super::Route::Crashes)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_crashes(cx);
                },
            ))
            .child(self.sidebar_row(
                "side-diagnostics",
                "运行诊断",
                matches!(self.stack.last(), Some(super::Route::Diagnostics)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_diagnostics(cx);
                },
            ))
            .child(self.sidebar_row(
                "side-blocklist",
                "屏蔽管理",
                self.blocklist_open,
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.blocklist = this.services.block_keywords();
                    this.blocklist_open = true;
                    let _ = cx;
                },
            ));

        let browse_toggle = div()
            .id("sidebar-browse-toggle")
            .aria_label("浏览分组")
            .h(px(24.))
            .px_1()
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_browse_open = !this.sidebar_browse_open;
                cx.notify();
            }))
            .child(div().text_xs().text_color(p.muted).child("浏览"))
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(if self.sidebar_browse_open {
                        "▾"
                    } else {
                        "▸"
                    }),
            );
        let my_toggle = div()
            .id("sidebar-my-toggle")
            .aria_label("我的分组")
            .h(px(24.))
            .px_1()
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_my_open = !this.sidebar_my_open;
                cx.notify();
            }))
            .child(div().text_xs().text_color(p.muted).child("我的"))
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(if self.sidebar_my_open { "▾" } else { "▸" }),
            );

        let mut browse_section = div().flex().flex_col().gap_2().child(browse_toggle);
        if self.sidebar_browse_open {
            browse_section = browse_section.child(nav);
        }
        let mut my_section = div().flex().flex_col().gap_2().child(my_toggle);
        if self.sidebar_my_open {
            my_section = my_section.child(actions);
        }

        let collapsed = self.sidebar_collapsed;
        let mut mini = div().flex().flex_col().gap_1();
        for (index, label) in NAV.iter().enumerate() {
            let active = self.tab == index && self.stack.is_empty() && !self.blocklist_open;
            let short: String = label.chars().take(1).collect();
            mini = mini.child(self.mini_row(
                MINI_NAV_IDS[index],
                label,
                &short,
                active,
                p,
                cx,
                move |this, cx| this.switch_tab(index, cx),
            ));
        }
        mini = mini
            .child(self.mini_row(
                "mini-notifications",
                "消息",
                "信",
                matches!(self.stack.last(), Some(super::Route::Notifications)),
                p,
                cx,
                |this, _| {
                    this.go_root();
                    this.open_notifications();
                },
            ))
            .child(self.mini_row(
                "mini-history",
                "阅读历史",
                "史",
                matches!(self.stack.last(), Some(super::Route::History)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_history(cx);
                },
            ))
            .child(self.mini_row(
                "mini-crashes",
                "崩溃日志",
                "崩",
                matches!(self.stack.last(), Some(super::Route::Crashes)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_crashes(cx);
                },
            ))
            .child(self.mini_row(
                "mini-diagnostics",
                "运行诊断",
                "诊",
                matches!(self.stack.last(), Some(super::Route::Diagnostics)),
                p,
                cx,
                |this, cx| {
                    this.go_root();
                    this.open_diagnostics(cx);
                },
            ))
            .child(self.mini_row(
                "mini-blocklist",
                "屏蔽管理",
                "屏",
                self.blocklist_open,
                p,
                cx,
                |this, _| {
                    this.go_root();
                    this.blocklist = this.services.block_keywords();
                    this.blocklist_open = true;
                },
            ));

        let mut brand = div()
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .child(div().size(px(22.)).rounded_md().bg(p.accent));
        if !collapsed {
            brand = brand
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.fg)
                        .child("知乎"),
                )
                .child(
                    div()
                        .px_1()
                        .py_0p5()
                        .rounded_sm()
                        .bg(p.elevated)
                        .text_xs()
                        .text_color(p.muted)
                        .child("Rust"),
                );
        }
        brand = brand.child(div().flex_1()).child(
            div()
                .id("sidebar-collapse")
                .aria_label(if collapsed {
                    "展开侧栏"
                } else {
                    "收起侧栏"
                })
                .size(px(22.))
                .rounded_sm()
                .bg(p.elevated)
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(p.muted)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar_collapsed = !this.sidebar_collapsed;
                    cx.notify();
                }))
                .child(if collapsed { "»" } else { "«" }),
        );

        let mut account = div()
            .id("side-account")
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .rounded_md()
            .bg(p.elevated)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.switch_tab(4, cx);
            }))
            .child(
                div()
                    .size(px(28.))
                    .rounded_full()
                    .bg(if logged_in { p.accent } else { p.border })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if logged_in { p.accent_text } else { p.muted })
                            .child(SharedString::from(avatar)),
                    ),
            );
        if !collapsed {
            account = account.child(
                div()
                    .flex_col()
                    .child(div().text_xs().text_color(p.fg).child(if logged_in {
                        SharedString::from(account_label)
                    } else {
                        SharedString::from("未登录")
                    }))
                    .child(div().text_xs().text_color(p.muted).child(if logged_in {
                        "点击进入我的"
                    } else {
                        "Cookie / 扫码 / 手机号"
                    })),
            );
        }

        let mut sidebar = div()
            .w(px(if collapsed { 72. } else { 268. }))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .px(if collapsed { px(8.) } else { px(16.) })
            .py_5()
            .bg(p.surface)
            .border_r_1()
            .border_color(p.border)
            .child(brand);
        if collapsed {
            sidebar = sidebar.child(mini);
        } else {
            sidebar = sidebar
                .child(
                    Input::new(&self.search)
                        .h(px(38.))
                        .rounded_md()
                        .bg(p.elevated)
                        .text_color(p.fg),
                )
                .child(browse_section)
                .child(my_section);
        }
        sidebar = sidebar.child(div().flex_1()).child(account);
        if !collapsed {
            sidebar = sidebar.child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .text_xs()
                    .text_color(p.muted)
                    .px_1()
                    .child("Ctrl+K 搜索 · Ctrl+1–5 切换 Tab")
                    .child("Ctrl+R 刷新 · Ctrl+D 主题 · Esc 返回"),
            );
        }
        sidebar.into_any_element()
    }

    fn sidebar_row<F>(
        &self,
        id: &'static str,
        label: &str,
        active: bool,
        p: Palette,
        cx: &mut Context<Self>,
        action: F,
    ) -> AnyElement
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        div()
            .id(id)
            .aria_label(format!("{label} 导航"))
            .aria_selected(active)
            .h(px(38.))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .bg(if active { p.elevated } else { p.surface })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this, cx);
                cx.notify();
            }))
            .child(div().h(px(16.)).w(px(3.)).rounded_full().bg(if active {
                p.accent
            } else {
                p.surface
            }))
            .child(
                div()
                    .text_sm()
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if active { p.fg } else { p.muted })
                    .child(SharedString::from(label.to_string())),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn mini_row<F>(
        &self,
        id: &'static str,
        label: &str,
        short: &str,
        active: bool,
        p: Palette,
        cx: &mut Context<Self>,
        action: F,
    ) -> AnyElement
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        div()
            .id(id)
            .aria_label(label.to_string())
            .aria_selected(active)
            .h(px(38.))
            .w_full()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .bg(if active { p.elevated } else { p.surface })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this, cx);
                cx.notify();
            }))
            .child(
                div()
                    .text_sm()
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if active { p.accent } else { p.muted })
                    .child(SharedString::from(short.to_string())),
            )
            .into_any_element()
    }

    /// Wraps a routed page with the desktop sidebar. Returns the content
    /// unchanged on mobile or narrow windows.
    pub(crate) fn render_sidebar_shell(
        &mut self,
        window: &mut Window,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !wide_layout(window) {
            return content;
        }
        let p = palette(self.dark);
        let sidebar = self.render_sidebar(cx, p);
        div()
            .size_full()
            .flex()
            .bg(p.bg)
            .text_color(p.fg)
            .font_family(UI_FONT)
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.handle_shortcut(event, window, cx) {
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .child(sidebar)
            .child(div().flex_1().min_w_0().h_full().child(content))
            .into_any_element()
    }

    fn render_feed_list(&mut self, cx: &mut Context<Self>, p: Palette, wide: bool) -> AnyElement {
        let using_real_feed = !self.feed.is_empty();
        let count = if using_real_feed {
            self.feed.len()
        } else {
            self.items.len()
        };
        if self.loading_feed && count == 0 {
            return empty_state("加载中…", p.muted).into_any_element();
        }
        if count == 0 {
            return empty_state("暂无内容", p.muted).into_any_element();
        }
        let entity = cx.entity().downgrade();
        let items = if using_real_feed {
            &self.feed
        } else {
            &self.items
        };
        let column_count = if wide { 2 } else { 1 };
        let mut columns: Vec<Vec<AnyElement>> = (0..column_count).map(|_| Vec::new()).collect();
        let mut heights = vec![0.0f32; column_count];
        for item in items {
            let column = heights
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.total_cmp(b))
                .map(|(index, _)| index)
                .unwrap_or(0);
            heights[column] += feed_card_height(item);
            columns[column].push(feed_card(
                item,
                &entity,
                p.surface,
                p.fg,
                p.muted,
                p.border,
                p.accent,
                self.card_offset(&item.id),
            ));
        }

        let mut row = div().flex().items_start().gap_4().px_4().pb_4();
        for column in columns {
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(column),
            );
        }

        let loadable =
            using_real_feed && !self.feed_end && !self.feed_loading_more && !self.loading_feed;
        let footer_text = if !using_real_feed {
            "示例数据".to_string()
        } else if self.feed_loading_more {
            "加载中…".to_string()
        } else if self.feed_end {
            "没有更多了".to_string()
        } else if std::env::var("ZHIHU_SCROLL_DEBUG").is_ok() {
            let offset = f32::from(self.feed_scroll.offset().y);
            let max = f32::from(self.feed_scroll.max_offset().y);
            let viewport = f32::from(self.feed_scroll.bounds().size.height);
            format!(
                "off={offset:.0} max={max:.0} vp={viewport:.0} page={} g={} p={} po={:.0}",
                self.feed_page, self.gesture_pending, self.pull_active, self.pull_offset
            )
        } else {
            "继续滚动加载更多（点击也可加载）".to_string()
        };
        let mut footer = div()
            .id("feed-load-more")
            .h(px(52.))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(if loadable { p.accent } else { p.muted })
            .child(SharedString::from(footer_text));
        if loadable {
            footer = footer
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.load_more_feed(cx);
                }));
        }

        let scroll = div()
            .id(SharedString::from(format!(
                "feed-scroll-{}",
                self.list_epoch
            )))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.feed_scroll)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    this.start_feed_gesture(
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                }),
            )
            .child(row)
            .child(footer);

        if using_real_feed {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.render_pull_bar(p))
                .child(scroll)
                .into_any_element()
        } else {
            scroll.into_any_element()
        }
    }

    fn render_pull_bar(&self, p: Palette) -> AnyElement {
        let label = if self.pull_active {
            if self.pull_offset >= 50.0 {
                "松开刷新"
            } else {
                "下拉刷新"
            }
        } else {
            "↓ 下拉刷新（条目区域任意位置向下拖动）"
        };
        let height = 30.0 + self.pull_offset.min(60.0);
        div()
            .id("feed-pull")
            .h(px(height))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(if self.pull_offset >= 50.0 {
                p.accent
            } else {
                p.muted
            })
            .child(label)
            .into_any_element()
    }

    fn render_hot_list(&mut self, cx: &mut Context<Self>, p: Palette) -> AnyElement {
        if self.hot.is_empty() {
            return empty_state(
                if self.loading_hot {
                    "加载中…"
                } else {
                    "暂无热榜数据"
                },
                p.muted,
            )
            .into_any_element();
        }
        let count = self.hot.len();
        let list_id = format!("home-hot-{}", self.list_epoch);
        let entity = cx.entity().downgrade();
        uniform_list(
            list_id,
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    rows.push(hot_row(
                        index,
                        &this.hot[index],
                        &entity,
                        p.surface,
                        p.fg,
                        p.muted,
                        p.border,
                        p.accent,
                    ));
                }
                rows
            }),
        )
        .size_full()
        .into_any_element()
    }

    fn render_daily_list(&mut self, cx: &mut Context<Self>, p: Palette) -> AnyElement {
        if self.daily.is_empty() {
            return empty_state(
                if self.loading_daily {
                    "加载中…"
                } else {
                    "暂无日报数据"
                },
                p.muted,
            )
            .into_any_element();
        }
        let count = self.daily.len();
        let list_id = format!("home-daily-{}", self.list_epoch);
        let entity = cx.entity().downgrade();
        uniform_list(
            list_id,
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    rows.push(daily_row(
                        &this.daily[index],
                        &entity,
                        p.surface,
                        p.fg,
                        p.muted,
                        p.border,
                    ));
                }
                rows
            }),
        )
        .size_full()
        .into_any_element()
    }

    fn render_profile(&mut self, cx: &mut Context<Self>, p: &Palette) -> AnyElement {
        let logged_in = self.is_logged_in();
        let name = self
            .account
            .as_ref()
            .map(|account| account.name.clone())
            .unwrap_or_else(|| {
                if logged_in {
                    "已登录".to_string()
                } else {
                    "未登录".to_string()
                }
            });
        let profile_avatar = if logged_in {
            name.chars().next().unwrap_or('我').to_string()
        } else {
            "我".to_string()
        };
        let subtitle = self
            .account
            .as_ref()
            .and_then(|account| account.headline.clone())
            .unwrap_or_else(|| {
                if logged_in {
                    "建议：在电脑浏览器登录知乎后复制 Cookie".to_string()
                } else {
                    "导入 Cookie 后可查看推荐流、点赞、关注和评论".to_string()
                }
            });

        let account_card = div()
            .w_full()
            .flex_shrink_0()
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
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
                    .child(
                        div()
                            .size(px(52.))
                            .rounded_full()
                            .bg(if logged_in { p.accent } else { p.elevated })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if logged_in { p.accent_text } else { p.muted })
                                    .child(SharedString::from(profile_avatar)),
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
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.fg)
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(SharedString::from(subtitle)),
                            ),
                    )
                    .child(
                        div()
                            .id("profile-login")
                            .flex_shrink_0()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(if logged_in { p.elevated } else { p.accent })
                            .text_sm()
                            .text_color(if logged_in { p.muted } else { p.accent_text })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.login_open = !this.login_open;
                                this.blocklist_open = false;
                                cx.notify();
                            }))
                            .child(if logged_in { "切换账号" } else { "登录" }),
                    ),
            )
            .child(if self.login_open {
                self.render_login_form(cx, p)
            } else {
                div()
            });

        let setting_row = |label: &'static str, value: String| {
            div()
                .w_full()
                .px_4()
                .py_3()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(p.border)
                .child(div().text_sm().text_color(p.fg).child(label))
                .child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(SharedString::from(value)),
                )
        };

        let theme_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(div().text_sm().text_color(p.fg).child("深色模式"))
            .child(
                div()
                    .id("profile-theme")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if self.dark { p.accent } else { p.elevated })
                    .text_xs()
                    .text_color(if self.dark { p.accent_text } else { p.muted })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_theme();
                        cx.notify();
                    }))
                    .child(if self.dark { "开" } else { "关" }),
            );

        let font_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(div().text_sm().text_color(p.fg).child("正文字号"))
            .child(
                div().flex().gap_2().children(
                    [(14.0f32, "小"), (16.0, "中"), (19.0, "大")]
                        .into_iter()
                        .map(|(size, label)| {
                            let active = (self.font_size - size).abs() < 0.1;
                            div()
                                .id(SharedString::from(format!("font-size-{label}")))
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .bg(if active { p.accent } else { p.elevated })
                                .text_xs()
                                .text_color(if active { p.accent_text } else { p.muted })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_font_size(size);
                                    cx.notify();
                                }))
                                .child(label)
                        }),
                ),
            );

        let line_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(div().text_sm().text_color(p.fg).child("行距"))
            .child(
                div().flex().gap_2().children(
                    [(24.0f32, "紧凑"), (28.0, "标准"), (32.0, "宽松")]
                        .into_iter()
                        .map(|(height, label)| {
                            let active = (self.line_height - height).abs() < 0.1;
                            div()
                                .id(SharedString::from(format!("line-height-{label}")))
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .bg(if active { p.accent } else { p.elevated })
                                .text_xs()
                                .text_color(if active { p.accent_text } else { p.muted })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_line_height(height);
                                    cx.notify();
                                }))
                                .child(label)
                        }),
                ),
            );

        let typography_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_sm().text_color(p.fg).child("排版"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child(self.typography_preset.description()),
                    ),
            )
            .child(
                div().flex().gap_2().children(
                    [
                        TypographyPreset::Standard,
                        TypographyPreset::Relaxed,
                        TypographyPreset::Compact,
                    ]
                    .into_iter()
                    .map(|preset| {
                        let active = self.typography_preset == preset;
                        div()
                            .id(SharedString::from(format!(
                                "typography-{}",
                                preset.as_str()
                            )))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(if active { p.accent } else { p.elevated })
                            .text_xs()
                            .text_color(if active { p.accent_text } else { p.muted })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_typography_preset(preset, cx);
                            }))
                            .child(preset.label())
                    }),
                ),
            );

        let preview = self.typography();
        let typography_preview = div()
            .w_full()
            .px_4()
            .pb_3()
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .w_full()
                    .rounded_lg()
                    .bg(p.elevated)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(preview.heading_size(2)))
                            .line_height(px(preview.heading_line_height(2)))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.fg)
                            .child("排版预览"),
                    )
                    .child(
                        div()
                            .text_size(px(preview.body_size))
                            .line_height(px(preview.line_height))
                            .text_color(p.fg)
                            .child("好的阅读体验来自合适的栏宽、行高与留白。切换预设会立即应用到回答与文章正文。"),
                    )
                    .child(
                        div()
                            .pl(px(preview.quote_padding))
                            .border_l_3()
                            .border_color(p.accent)
                            .text_size(px(preview.body_size * 0.92))
                            .line_height(px(preview.line_height * 0.92))
                            .text_color(p.muted)
                            .child("引用：层级与留白让长文更易读。"),
                    ),
            );

        let recommend_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_sm().text_color(p.fg).child("推荐排序"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child("仅使用本机阅读/点赞/关注记录"),
                    ),
            )
            .child(
                div().flex().gap_2().children(
                    [
                        (super::RecommendMode::Off, "关闭"),
                        (super::RecommendMode::Hybrid, "混合"),
                        (super::RecommendMode::Local, "本地"),
                    ]
                    .into_iter()
                    .map(|(mode, label)| {
                        let active = self.recommend_mode == mode;
                        div()
                            .id(SharedString::from(format!("recommend-{}", mode.as_str())))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(if active { p.accent } else { p.elevated })
                            .text_xs()
                            .text_color(if active { p.accent_text } else { p.muted })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_recommend_mode(mode, cx);
                            }))
                            .child(label)
                    }),
                ),
            );

        let recommend_data_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .child(div().text_sm().text_color(p.fg).child("推荐数据"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child(SharedString::from(format!(
                                "{} 条",
                                self.recommend_feedback_count
                            ))),
                    )
                    .child(
                        div()
                            .id("recommend-clear")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(p.elevated)
                            .text_xs()
                            .text_color(p.danger)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_recommendation_data(cx);
                            }))
                            .child("清除"),
                    ),
            );

        let block_row = div()
            .id("profile-blocklist")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.blocklist = this.services.block_keywords();
                this.blocklist_open = true;
                cx.notify();
            }))
            .child(div().text_sm().text_color(p.fg).child("屏蔽词"))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(SharedString::from(format!("{} 条", self.blocklist.len()))),
            );

        let history_row = div()
            .id("profile-history")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_history(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("阅读历史"))
            .child(div().text_sm().text_color(p.muted).child("查看"));

        let crash_row = div()
            .id("profile-crashes")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_crashes(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("崩溃日志"))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(SharedString::from(format!("{} 份", self.crash_logs.len()))),
            );

        let diag_row = div()
            .id("profile-diagnostics")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_diagnostics(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("运行诊断"))
            .child(div().text_sm().text_color(p.muted).child("查看"));

        let tts_row = div()
            .id("profile-tts")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_tts_settings(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("朗读设置"))
            .child(div().text_sm().text_color(p.muted).child("查看"));

        let notification_row = div()
            .id("profile-notifications")
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(p.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_notifications();
                cx.notify();
            }))
            .child(div().text_sm().text_color(p.fg).child("消息中心"))
            .child(div().text_sm().text_color(p.muted).child("查看"));

        let settings = div()
            .w_full()
            .flex_shrink_0()
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .rounded_lg()
            .overflow_hidden()
            .child(theme_row)
            .child(font_row)
            .child(line_row)
            .child(typography_row)
            .child(typography_preview)
            .child(recommend_row)
            .child(recommend_data_row)
            .child(block_row)
            .child(history_row)
            .child(crash_row)
            .child(diag_row)
            .child(tts_row)
            .child(notification_row)
            .child(setting_row(
                "关于",
                format!("知乎 Rust · M5 · {}", env!("CARGO_PKG_VERSION")),
            ));

        div()
            .id("profile-scroll")
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(account_card)
            .child(settings)
            .into_any_element()
    }

    fn render_login_form(&self, cx: &mut Context<Self>, p: &Palette) -> Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("手机号登录（短信验证码）"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.phone_input)
                            .flex_1()
                            .h(px(38.))
                            .bg(p.elevated)
                            .text_color(p.fg),
                    )
                    .child(
                        div()
                            .id("phone-send")
                            .flex_shrink_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(p.elevated)
                            .text_sm()
                            .text_color(p.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let phone = this.phone_input.read(cx).value().trim().to_string();
                                if phone.is_empty() {
                                    this.phone_status = Some("请输入手机号".to_string());
                                } else {
                                    this.phone_captcha = None;
                                    this.phone_status = Some("正在发送…".to_string());
                                    this.services.phone_request_code(phone);
                                }
                                cx.notify();
                            }))
                            .child("发送验证码"),
                    ),
            )
            .child(if self.phone_captcha_required {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(
                        self.phone_captcha
                            .as_ref()
                            .map(|path| img(path.clone()).h(px(44.)).into_any_element()),
                    )
                    .child(
                        Input::new(&self.captcha_input)
                            .flex_1()
                            .h(px(38.))
                            .bg(p.elevated)
                            .text_color(p.fg),
                    )
                    .child(
                        div()
                            .id("phone-captcha-verify")
                            .flex_shrink_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(p.elevated)
                            .text_sm()
                            .text_color(p.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let input = this.captcha_input.read(cx).value().trim().to_string();
                                this.phone_status = Some("正在验证图形验证码…".to_string());
                                this.services.phone_verify_captcha(input);
                                cx.notify();
                            }))
                            .child("验证"),
                    )
            } else {
                div()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.sms_input)
                            .flex_1()
                            .h(px(38.))
                            .bg(p.elevated)
                            .text_color(p.fg),
                    )
                    .child(
                        div()
                            .id("phone-sign-in")
                            .flex_shrink_0()
                            .px_4()
                            .py_2()
                            .rounded_lg()
                            .bg(p.accent)
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.accent_text)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let phone = this.phone_input.read(cx).value().trim().to_string();
                                let code = this.sms_input.read(cx).value().trim().to_string();
                                if phone.is_empty() || code.is_empty() {
                                    this.phone_status =
                                        Some("请先输入手机号和短信验证码".to_string());
                                } else {
                                    this.phone_status = Some("正在登录…".to_string());
                                    this.services.phone_sign_in(phone, code);
                                }
                                cx.notify();
                            }))
                            .child("登录"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if self.login_invalid {
                        p.danger
                    } else {
                        p.muted
                    })
                    .child(SharedString::from(
                        self.phone_status.clone().unwrap_or_default(),
                    )),
            )
            .child(div().h(px(1.)).w_full().bg(p.border))
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("粘贴浏览器 Cookie（例如 z_c0=...; d_c0=...;），仅保存在本机。"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("qr-login")
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(p.elevated)
                            .text_sm()
                            .text_color(p.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.qr_matrix = None;
                                this.qr_status = Some("正在获取二维码…".to_string());
                                this.services.start_qr_login();
                                cx.notify();
                            }))
                            .child("扫码登录"),
                    )
                    .child(div().text_xs().text_color(p.muted).child("或粘贴 Cookie")),
            )
            .child(
                Input::new(&self.cookie_input)
                    .w_full()
                    .h(px(38.))
                    .bg(p.elevated)
                    .text_color(p.fg),
            )
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("cookie-import")
                        .px_4()
                        .py_2()
                        .rounded_lg()
                        .bg(p.accent)
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.accent_text)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            let raw = this.cookie_input.read(cx).value().to_string();
                            let count = this.services.import_cookie_header(&raw);
                            this.login_status = Some(if count > 0 {
                                format!("已导入 {count} 个 Cookie，正在校验…")
                            } else {
                                "未识别到 Cookie".to_string()
                            });
                            if count > 0 {
                                this.loading_feed = true;
                                this.services.validate_login();
                                this.services.refresh_feed();
                            }
                            cx.notify();
                        }))
                        .child("导入并校验"),
                ),
            )
            .child(if let Some((message, redirect)) = &self.qr_risk {
                let target = redirect
                    .clone()
                    .unwrap_or_else(|| "https://www.zhihu.com/account/risk_control/".to_string());
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .bg(p.elevated)
                    .border_1()
                    .border_color(p.danger)
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.danger)
                            .child(SharedString::from(message.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("qr-risk-open")
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(p.accent)
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.accent_text)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        open_external(&target);
                                        cx.stop_propagation();
                                    }))
                                    .child("打开验证页面"),
                            )
                            .child(
                                div()
                                    .id("qr-risk-retry")
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(p.surface)
                                    .text_sm()
                                    .text_color(p.accent)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.qr_risk = None;
                                        this.qr_status = Some("正在重试获取二维码…".to_string());
                                        this.services.start_qr_login();
                                        cx.notify();
                                    }))
                                    .child("重试"),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child("验证后若仍失败，请切换网络，或改用手机号登录 / Cookie 导入。"),
                    )
            } else if let Some(matrix) = &self.qr_matrix {
                qr_view(matrix)
            } else {
                div()
            })
            .child(if self.qr_risk.is_some() {
                div()
            } else {
                div()
                    .text_xs()
                    .text_color(if self.login_invalid {
                        p.danger
                    } else {
                        p.muted
                    })
                    .child(SharedString::from(
                        self.qr_status
                            .clone()
                            .or_else(|| self.login_status.clone())
                            .unwrap_or_default(),
                    ))
            })
    }

    fn render_blocklist_panel(&mut self, cx: &mut Context<Self>, p: &Palette) -> AnyElement {
        let mut rows = div().flex().flex_col().gap_2();
        for (pattern, is_regex) in &self.blocklist {
            let label = format!("{}{}", if *is_regex { "正则: " } else { "" }, pattern);
            let pattern_for_click = pattern.clone();
            rows = rows.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(p.elevated)
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.fg)
                            .child(SharedString::from(label)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("block-del-{pattern}")))
                            .text_sm()
                            .text_color(p.danger)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services
                                    .remove_block_keyword(pattern_for_click.clone());
                                this.blocklist = this.services.block_keywords();
                                this.blocklist_status = Some("已删除".to_string());
                                this.refresh_after_blocklist();
                                cx.notify();
                            }))
                            .child("删除"),
                    ),
            );
        }

        let mut user_rows = div().flex().flex_col().gap_2();
        for (id, name) in &self.blocked_users {
            let id_for_click = id.clone();
            user_rows = user_rows.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(p.elevated)
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.fg)
                            .child(SharedString::from(name.clone())),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("unblock-user-{id}")))
                            .text_sm()
                            .text_color(p.danger)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services.unblock_user(id_for_click.clone());
                                this.blocked_users = this.services.blocked_users();
                                this.blocklist_status = Some("已解除用户屏蔽".to_string());
                                this.refresh_after_blocklist();
                                cx.notify();
                            }))
                            .child("解除"),
                    ),
            );
        }

        let mut topic_rows = div().flex().flex_col().gap_2();
        for (id, name) in &self.blocked_topics {
            let id_for_click = id.clone();
            topic_rows = topic_rows.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(p.elevated)
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.fg)
                            .child(SharedString::from(name.clone())),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("unblock-topic-{id}")))
                            .text_sm()
                            .text_color(p.danger)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.services.unblock_topic(id_for_click.clone());
                                this.blocked_topics = this.services.blocked_topics();
                                this.blocklist_status = Some("已解除话题屏蔽".to_string());
                                this.refresh_after_blocklist();
                                cx.notify();
                            }))
                            .child("解除"),
                    ),
            );
        }
        let topic_add = div()
            .flex()
            .gap_2()
            .child(
                Input::new(&self.topic_input)
                    .flex_1()
                    .h(px(38.))
                    .bg(p.elevated)
                    .text_color(p.fg),
            )
            .child(
                div()
                    .id("topic-add")
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(p.accent)
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let topic = this.topic_input.read(cx).value().trim().to_string();
                        if !topic.is_empty() {
                            this.services.block_topic(topic.clone(), topic);
                            this.blocked_topics = this.services.blocked_topics();
                            this.blocklist_status = Some("已屏蔽话题".to_string());
                            this.refresh_after_blocklist();
                            cx.notify();
                        }
                    }))
                    .child("添加"),
            );

        div()
            .id("home-blocklist-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.fg)
                            .child("屏蔽词"),
                    )
                    .child(
                        div()
                            .id("block-close")
                            .text_sm()
                            .text_color(p.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.blocklist_open = false;
                                cx.notify();
                            }))
                            .child("关闭"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("匹配标题、摘要或作者名；命中后从热榜、日报和推荐流中移除。"),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Input::new(&self.blocklist_input)
                            .flex_1()
                            .h(px(38.))
                            .bg(p.elevated)
                            .text_color(p.fg),
                    )
                    .child(
                        div()
                            .id("block-add")
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(p.accent)
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.accent_text)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let pattern =
                                    this.blocklist_input.read(cx).value().trim().to_string();
                                if !pattern.is_empty() {
                                    this.services.add_block_keyword(pattern, false);
                                    this.blocklist = this.services.block_keywords();
                                    this.blocklist_status = Some("已添加".to_string());
                                    this.refresh_after_blocklist();
                                    cx.notify();
                                }
                            }))
                            .child("添加"),
                    ),
            )
            .child(rows)
            .child(div().text_sm().text_color(p.muted).child("已屏蔽用户"))
            .child(user_rows)
            .child(div().text_sm().text_color(p.muted).child("屏蔽话题"))
            .child(topic_add)
            .child(topic_rows)
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(SharedString::from(
                        self.blocklist_status.clone().unwrap_or_default(),
                    )),
            )
            .into_any_element()
    }
}

fn qr_view(matrix: &[Vec<bool>]) -> Div {
    let mut rows = div().flex().flex_col();
    for row in matrix {
        let mut cells = div().flex();
        for dark in row {
            cells = cells.child(div().w(px(6.)).h(px(6.)).bg(if *dark {
                rgb(0x000000)
            } else {
                rgb(0xffffff)
            }));
        }
        rows = rows.child(cells);
    }
    div()
        .w_full()
        .flex()
        .justify_center()
        .py_2()
        .child(div().bg(rgb(0xffffff)).p_2().child(rows))
}
