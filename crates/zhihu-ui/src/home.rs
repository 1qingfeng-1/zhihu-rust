//! Binance-inspired mobile home: dark-first palette, yellow accent, bottom
//! navigation and compact content cards.

use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, img, px, rgb, Animation, AnimationExt as _, AnyElement, Context, Div, Focusable,
    FontWeight, MouseButton, Rgba, SharedString, Window,
};
use zhihu_types::{FeedItem, FollowFeedKind};

#[cfg(not(target_os = "android"))]
use super::open_external;
use super::{
    daily_row, empty_state, feed_card, hot_row, safe_area, TypographyPreset, ZhihuApp, TABS,
    UI_FONT,
};

fn unread_badge_label(count: u64) -> String {
    if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    }
}

fn format_daily_date(date: &str) -> String {
    if date.len() == 8 && date.chars().all(|ch| ch.is_ascii_digit()) {
        format!("{} 年 {} 月 {} 日", &date[0..4], &date[4..6], &date[6..8])
    } else {
        date.to_string()
    }
}

fn feed_card_height(item: &FeedItem) -> f32 {
    let title_lines = item.title.chars().count().div_ceil(18).max(1);
    let excerpt_lines = item.excerpt.chars().count().div_ceil(24).clamp(1, 3);
    let topic_height = if item.topic_refs.is_empty() {
        0.0
    } else {
        26.0
    };
    112.0 + title_lines as f32 * 24.0 + excerpt_lines as f32 * 20.0 + topic_height
}

fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Publish time for the timeline. Local-pool and sample items carry no
/// timestamp, so they get a stable pseudo time derived from their id instead
/// of shifting on every refresh.
fn feed_item_time(item: &FeedItem) -> i64 {
    if item.created_at > 0 {
        return item.created_at;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    item.id.hash(&mut hasher);
    let hours = (hasher.finish() % 72) as i64;
    unix_now_secs() - hours * 3_600 - 600
}

fn relative_time_label(timestamp: i64) -> String {
    let delta = (unix_now_secs() - timestamp).max(0);
    match delta {
        0..=59 => "刚刚".to_string(),
        60..=3_599 => format!("{}分钟前", delta / 60),
        3_600..=86_399 => format!("{}小时前", delta / 3_600),
        86_400..=172_799 => "昨天".to_string(),
        172_800..=604_799 => format!("{}天前", delta / 86_400),
        _ => format!("{}周前", delta / 604_800),
    }
}

/// One mobile feed cell on the timeline: a rail with a dot and publish time,
/// then the card. The rail line bleeds past the row gap so the list reads as
/// one continuous line while scrolling back through history.
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
            .child(self.search_pill(p, cx, true))
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
                            .child("消息")
                            .children((self.notification_unread > 0).then(|| {
                                div()
                                    .px_1()
                                    .rounded_full()
                                    .bg(p.danger)
                                    .text_xs()
                                    .text_color(rgb(0xffffff))
                                    .child(SharedString::from(unread_badge_label(
                                        self.notification_unread,
                                    )))
                            })),
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
                            .child(if let Some(url) = self.avatar_url() {
                                img(url).size(px(30.)).rounded_full().into_any_element()
                            } else {
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if logged_in { p.accent_text } else { p.muted })
                                    .child(SharedString::from(avatar))
                                    .into_any_element()
                            }),
                    ),
            );

        let content: AnyElement = if self.blocklist_open {
            self.render_blocklist_panel(cx, &p)
        } else {
            match self.tab {
                0 => self.render_feed_list(cx, p, wide),
                1 => self.render_follow_list(window, cx, p, wide),
                2 => self.render_hot_list(cx, p),
                3 => self.render_daily_list(cx, p),
                _ => self.render_profile(cx, &p),
            }
        };
        // Only the tab content follows the finger; the header, the bottom tab
        // bar and the 关注 sub-tabs stay put.
        let content = if self.tab == 1 && !self.blocklist_open {
            content
        } else {
            self.slide_wrap(content, window, p.bg)
        };

        let tab_error = match self.tab {
            0 => self.feed_error.as_ref(),
            1 => self.follow_state().error.as_ref(),
            2 => self.hot_error.as_ref(),
            3 => self.daily_error.as_ref(),
            _ => self.error.as_ref(),
        };
        let status_text = if let Some(error) = tab_error {
            error.clone()
        } else {
            match self.tab {
                0 => {
                    let count = if self.feed.is_empty() {
                        self.items.len()
                    } else {
                        self.feed.len()
                    };
                    format!("推荐 · {count} 条")
                }
                1 => format!(
                    "关注 · {} · {} 条",
                    self.follow_tab.label(),
                    self.follow_state().items.len()
                ),
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
            );

        let nav = div()
            .h(px(56.))
            .bg(p.surface)
            .border_t_1()
            .border_color(p.border)
            .flex()
            .children(NAV.iter().enumerate().map(|(index, label)| {
                let active = self.tab == index;
                let icon = match index {
                    0 => gpui_kit::assets::IconName::House,
                    1 => gpui_kit::assets::IconName::Users,
                    2 => gpui_kit::assets::IconName::Flame,
                    3 => gpui_kit::assets::IconName::Newspaper,
                    _ => gpui_kit::assets::IconName::User,
                };
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
                    .gap_0p5()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_tab(index);
                        cx.notify();
                    }))
                    .child(Icon::new(icon).size(px(20.)).text_color(if active {
                        p.accent
                    } else {
                        p.muted
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
            let subtitle = if self.tab == 0 {
                format!("知乎 · Rust 客户端 · 来源：{}", self.feed_source.label())
            } else {
                "知乎 · Rust 客户端".to_string()
            };
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
                                .child(SharedString::from(subtitle.clone())),
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

    /// Wide search entry used in the header row: icon plus a vertically
    /// rotating hot-search label. Tapping opens the search page.
    fn search_pill(&self, p: Palette, cx: &mut Context<Self>, grow: bool) -> AnyElement {
        let hot = self
            .hot
            .get(self.hot_rotate_index)
            .map(|item| item.query.clone())
            .unwrap_or_else(|| "搜索知乎".to_string());
        let index = self.hot_rotate_index;
        let animation = Animation::new(std::time::Duration::from_millis(220))
            .with_easing(gpui_kit::base::animation::ease_out_cubic);
        let mut pill = div()
            .id("home-search-pill")
            .h(px(36.))
            .px_3()
            .rounded_full()
            .bg(p.elevated)
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                this.open_search_entry(cx);
                let handle = this.search.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                cx.notify();
            }))
            .child(
                Icon::new(IconName::Search)
                    .size(px(15.))
                    .text_color(p.muted),
            )
            .child(
                div().flex_1().min_w_0().h(px(20.)).overflow_hidden().child(
                    div()
                        .w_full()
                        .h_full()
                        .flex()
                        .items_center()
                        .with_animation(
                            SharedString::from(format!("hot-rotate-{index}")),
                            animation,
                            move |element, delta| element.mt(px(20.0 * (1.0 - delta))),
                        )
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_sm()
                                .text_color(p.muted)
                                .child(SharedString::from(hot)),
                        ),
                ),
            );
        if grow {
            pill = pill.flex_1().min_w_0().mx_3();
        } else {
            pill = pill.w_full();
        }
        pill.into_any_element()
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
                    .child(if let Some(url) = self.avatar_url() {
                        img(url).size(px(28.)).rounded_full().into_any_element()
                    } else {
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if logged_in { p.accent_text } else { p.muted })
                            .child(SharedString::from(avatar))
                            .into_any_element()
                    }),
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
                .child(self.search_pill(p, cx, false))
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
            .children(
                (id == "side-notifications" && self.notification_unread > 0).then(|| {
                    div()
                        .px_1()
                        .rounded_full()
                        .bg(p.danger)
                        .text_xs()
                        .text_color(rgb(0xffffff))
                        .child(SharedString::from(unread_badge_label(
                            self.notification_unread,
                        )))
                }),
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
                    .flex()
                    .flex_col()
                    .items_center()
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
                    .children(
                        (id == "mini-notifications" && self.notification_unread > 0).then(|| {
                            div()
                                .text_xs()
                                .text_color(p.danger)
                                .child(SharedString::from(unread_badge_label(
                                    self.notification_unread,
                                )))
                        }),
                    ),
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
            let read = self.is_read(item.kind, &item.id);
            let stamp = self
                .feed_times
                .get(&item.id)
                .copied()
                .unwrap_or_else(|| feed_item_time(item));
            let card = feed_card(
                item,
                &entity,
                p.surface,
                p.fg,
                p.muted,
                p.border,
                p.accent,
                self.card_offset(&item.id),
                wide,
                read,
                relative_time_label(stamp),
            );
            columns[column].push(card);
        }

        let mut row = if wide {
            div()
                .w_full()
                .flex()
                .items_start()
                .flex_shrink_0()
                .gap_4()
                .px_4()
                .pb_4()
        } else {
            div()
                .w_full()
                .flex()
                .items_start()
                .flex_shrink_0()
                .gap_3()
                .pb_4()
        };
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
            .flex_shrink_0()
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

        // Always bound the list to the remaining height: a bare `flex_1`
        // scroll container inside a non-flex parent takes its content height,
        // which makes `max_offset` zero and lets cards spill past the tab bar.
        let mut container = div().size_full().flex().flex_col();
        if using_real_feed {
            container = container.child(self.render_pull_bar(p));
        }
        container.child(scroll).into_any_element()
    }

    fn render_follow_list(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        p: Palette,
        wide: bool,
    ) -> AnyElement {
        let mut tabs = div()
            .id("follow-subtabs")
            .h(px(42.))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .bg(p.bg)
            .border_b_1()
            .border_color(p.border);
        for kind in [FollowFeedKind::Recommend, FollowFeedKind::Activity] {
            let active = self.follow_tab == kind;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("follow-tab-{}", kind.label())))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_b_1()
                    .border_color(if active { p.accent } else { p.border })
                    .text_sm()
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if active { p.fg } else { p.muted })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_follow_tab(kind, cx);
                    }))
                    .child(kind.label()),
            );
        }

        let body = self.render_follow_body(cx, p, wide);
        // Only the feed below the 推荐/动态 bar slides; the bar itself is fixed.
        let body = self.slide_wrap(body, window, p.bg);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(tabs)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_follow_body(&mut self, cx: &mut Context<Self>, p: Palette, wide: bool) -> AnyElement {
        let kind = self.follow_tab;
        let index = super::follow_index(kind);
        let state = &self.follow_feeds[index];
        let count = state.items.len();
        if !self.is_logged_in() {
            return empty_state("登录后可查看关注内容", p.muted).into_any_element();
        }
        if state.loading && count == 0 {
            return empty_state("加载中…", p.muted).into_any_element();
        }
        if let Some(error) = &state.error {
            if count == 0 {
                return div()
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
                    .into_any_element();
            }
        }
        if count == 0 {
            return empty_state(
                if kind == FollowFeedKind::Recommend {
                    "暂无关注推荐"
                } else {
                    "暂无关注动态"
                },
                p.muted,
            )
            .into_any_element();
        }

        let entity = cx.entity().downgrade();
        let items = &self.follow_feeds[index].items;
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
            let read = self.is_read(item.kind, &item.id);
            let stamp = self
                .feed_times
                .get(&item.id)
                .copied()
                .unwrap_or_else(|| feed_item_time(item));
            let card = feed_card(
                item,
                &entity,
                p.surface,
                p.fg,
                p.muted,
                p.border,
                p.accent,
                self.card_offset(&item.id),
                wide,
                read,
                relative_time_label(stamp),
            );
            columns[column].push(card);
        }
        let mut row = if wide {
            div()
                .w_full()
                .flex()
                .items_start()
                .flex_shrink_0()
                .gap_4()
                .px_4()
                .pb_4()
        } else {
            div()
                .w_full()
                .flex()
                .items_start()
                .flex_shrink_0()
                .gap_3()
                .pb_4()
        };
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

        let state = &self.follow_feeds[index];
        let loadable = !state.is_end && !state.loading && !state.loading_more;
        let footer_text = if state.loading_more {
            "加载中…".to_string()
        } else if state.is_end {
            "没有更多了".to_string()
        } else {
            "继续滚动加载更多（点击也可加载）".to_string()
        };
        let mut footer = div()
            .id(SharedString::from(format!("follow-load-more-{index}")))
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
                    this.load_more_follow_feed(cx);
                }));
        }

        let scroll = div()
            .id(SharedString::from(format!(
                "follow-scroll-{index}-{}",
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_pull_bar(p))
            .child(scroll)
            .into_any_element()
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
        let entity = cx.entity().downgrade();
        // A plain scroll column rather than `uniform_list`: on Android the
        // virtualized list's rows never receive the tap, so hot terms looked
        // dead. The list is capped at 20 items, so there is nothing to
        // virtualize anyway.
        let mut column = div()
            .id(SharedString::from(format!("home-hot-{}", self.list_epoch)))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (index, item) in self.hot.iter().enumerate() {
            column = column.child(hot_row(
                index, item, &entity, p.surface, p.fg, p.muted, p.border, p.accent,
            ));
        }
        column.into_any_element()
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
        let entity = cx.entity().downgrade();
        let mut column = div()
            .id(SharedString::from(format!(
                "home-daily-{}",
                self.list_epoch
            )))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let mut current_date = String::new();
        for story in &self.daily {
            if story.date != current_date {
                current_date = story.date.clone();
                column = column.child(
                    div()
                        .w_full()
                        .px_4()
                        .pt_4()
                        .pb_1()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.fg)
                        .child(SharedString::from(format_daily_date(&current_date))),
                );
            }
            column = column.child(daily_row(
                story, &entity, p.surface, p.fg, p.muted, p.border,
            ));
        }
        if let Some(error) = &self.daily_error {
            column = column.child(
                div()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(p.danger)
                    .child(SharedString::from(error.clone())),
            );
        }
        if self.daily_end {
            column = column.child(
                div()
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(p.muted)
                    .child("没有更早的日报"),
            );
        } else {
            let date = self.daily.last().map(|story| story.date.clone());
            column = column.child(
                div()
                    .id("daily-load-more")
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(p.accent)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.daily_loading_more {
                            return;
                        }
                        let Some(date) = date.clone() else {
                            return;
                        };
                        this.daily_loading_more = true;
                        this.daily_error = None;
                        this.services.load_more_daily(date);
                        cx.notify();
                    }))
                    .child(if self.daily_loading_more {
                        "加载中…"
                    } else {
                        "加载更早的日报"
                    }),
            );
        }
        column.into_any_element()
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
                            .child(if let Some(url) = self.avatar_url() {
                                img(url).size(px(52.)).rounded_full().into_any_element()
                            } else {
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if logged_in { p.accent_text } else { p.muted })
                                    .child(SharedString::from(profile_avatar))
                                    .into_any_element()
                            }),
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
                                this.open_login(cx);
                            }))
                            .child(if logged_in { "切换账号" } else { "登录" }),
                    ),
            );

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

        let reading_options = div()
            .w_full()
            .flex_shrink_0()
            .child(
                div()
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
                            .child(div().text_sm().text_color(p.fg).child("首行缩进"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child("正文段落首行缩进两个全角字符"),
                            ),
                    )
                    .child(
                        div()
                            .id("reading-indent-toggle")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(if self.reading_indent {
                                p.accent
                            } else {
                                p.elevated
                            })
                            .text_xs()
                            .text_color(if self.reading_indent {
                                p.accent_text
                            } else {
                                p.muted
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_reading_indent();
                                cx.notify();
                            }))
                            .child(if self.reading_indent { "开" } else { "关" }),
                    ),
            )
            .child(
                div()
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
                            .child(div().text_sm().text_color(p.fg).child("词性着色"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child("名词 / 动词 / 形容词使用克制的阅读配色"),
                            ),
                    )
                    .child(
                        div()
                            .id("pos-colors-toggle")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(if self.pos_colors {
                                p.accent
                            } else {
                                p.elevated
                            })
                            .text_xs()
                            .text_color(if self.pos_colors {
                                p.accent_text
                            } else {
                                p.muted
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_pos_colors();
                                cx.notify();
                            }))
                            .child(if self.pos_colors { "开" } else { "关" }),
                    ),
            )
            .child(
                div()
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
                            .child(div().text_sm().text_color(p.fg).child("屏蔽 MCN 推荐"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child("推荐流中隐藏已识别为 MCN 机构的作者"),
                            ),
                    )
                    .child(
                        div()
                            .id("hide-mcn-toggle")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(if self.hide_mcn { p.accent } else { p.elevated })
                            .text_xs()
                            .text_color(if self.hide_mcn {
                                p.accent_text
                            } else {
                                p.muted
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_hide_mcn();
                                cx.notify();
                            }))
                            .child(if self.hide_mcn { "开" } else { "关" }),
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

        let recommend_preset_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_sm().text_color(p.fg).child("推荐预设"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child(self.recommend_preset.description()),
                    ),
            )
            .child(div().w_full().flex().gap_2().children(
                super::RecommendPreset::ALL.into_iter().map(|preset| {
                    let active = self.recommend_preset == preset;
                    div()
                        .id(SharedString::from(format!(
                            "recommend-preset-{}",
                            preset.as_str()
                        )))
                        .flex_1()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_center()
                        .bg(if active { p.accent } else { p.elevated })
                        .text_xs()
                        .text_color(if active { p.accent_text } else { p.muted })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_recommend_preset(preset, cx);
                        }))
                        .child(preset.label())
                }),
            ));

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

        let collect_status = if self.recommend_collecting {
            "正在采集…".to_string()
        } else if let Some(report) = &self.recommend_last_report {
            if let Some(error) = &report.error {
                format!("上次失败：{error}")
            } else if report.cancelled {
                "上次已取消".to_string()
            } else {
                format!("上次采集 {} 条，新增 {}", report.fetched, report.inserted)
            }
        } else {
            "默认关闭，仅保存标题、摘要与计数".to_string()
        };
        let pool_text = format!(
            "候选池 {} 条 · 向量 {} 条 · 约 {} KB",
            self.recommend_pool.items,
            self.recommend_vectors,
            self.recommend_pool.approx_bytes / 1024
        );
        let collection_enabled = self.services.recommend_collection_enabled();
        let collect_toggle = div()
            .id("recommend-collect-toggle")
            .px_3()
            .py_1()
            .rounded_full()
            .bg(if collection_enabled {
                p.accent
            } else {
                p.elevated
            })
            .text_xs()
            .text_color(if collection_enabled {
                p.accent_text
            } else {
                p.muted
            })
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_recommend_collection(cx);
            }))
            .child(if collection_enabled { "开" } else { "关" });
        let collect_button = if self.recommend_collecting {
            div()
                .id("recommend-collect-cancel")
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(p.elevated)
                .text_xs()
                .text_color(p.muted)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cancel_recommend_collection(cx);
                }))
                .child("取消")
        } else {
            div()
                .id("recommend-collect-now")
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(if collection_enabled {
                    p.accent
                } else {
                    p.elevated
                })
                .text_xs()
                .text_color(if collection_enabled {
                    p.accent_text
                } else {
                    p.muted
                })
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.start_recommend_collection(cx);
                }))
                .child("立即采集")
        };
        let recommend_collect_row = div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex_col()
                            .gap_0p5()
                            .child(div().text_sm().text_color(p.fg).child("本地推荐采集"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(SharedString::from(collect_status)),
                            ),
                    )
                    .child(collect_toggle),
            )
            .child(
                div()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(SharedString::from(pool_text)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if self.recommend_embedding_fallback {
                                        p.danger
                                    } else {
                                        p.muted
                                    })
                                    .child(SharedString::from(
                                        if self.recommend_embedding_fallback {
                                            format!(
                                                "{}（ONNX 不可用，统计回退）",
                                                self.recommend_model
                                            )
                                        } else {
                                            self.recommend_model.clone()
                                        },
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(collect_button)
                            .child(
                                div()
                                    .id("recommend-pool-clear")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(p.elevated)
                                    .text_xs()
                                    .text_color(p.danger)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_recommend_pool(cx);
                                    }))
                                    .child("清除候选池"),
                            ),
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

        let collection_row = div()
            .id("profile-collections")
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
                this.open_collections(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("我的收藏"))
            .child(div().text_sm().text_color(p.muted).child("查看"));

        let notes_row = div()
            .id("profile-notes")
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
                this.open_notes(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child("阅读笔记"))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(SharedString::from(format!("{} 条", self.highlights.len()))),
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

        let login_row = div()
            .id("profile-login-row")
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
                this.open_login(cx);
            }))
            .child(div().text_sm().text_color(p.fg).child(if logged_in {
                "切换账号"
            } else {
                "登录"
            }))
            .child(div().text_sm().text_color(p.muted).child(if logged_in {
                "已登录"
            } else {
                "未登录"
            }));

        let account_group = self.profile_group(
            0,
            "账号与登录",
            "登录状态 · 消息中心",
            p,
            vec![
                login_row.into_any_element(),
                notification_row.into_any_element(),
            ],
            cx,
        );
        let content_group = self.profile_group(
            1,
            "我的内容",
            "收藏 · 笔记 · 历史",
            p,
            vec![
                collection_row.into_any_element(),
                notes_row.into_any_element(),
                history_row.into_any_element(),
            ],
            cx,
        );
        let recommend_group = self.profile_group(
            2,
            "推荐设置",
            "排序 · 预设 · 数据 · 采集",
            p,
            vec![
                recommend_row.into_any_element(),
                recommend_preset_row.into_any_element(),
                recommend_data_row.into_any_element(),
                recommend_collect_row.into_any_element(),
            ],
            cx,
        );
        let reading_group = self.profile_group(
            3,
            "阅读与外观",
            "深色 · 字号 · 行距 · 排版 · 朗读",
            p,
            vec![
                theme_row.into_any_element(),
                font_row.into_any_element(),
                line_row.into_any_element(),
                typography_row.into_any_element(),
                typography_preview.into_any_element(),
                reading_options.into_any_element(),
                tts_row.into_any_element(),
            ],
            cx,
        );
        let tools_group = self.profile_group(
            4,
            "其他工具",
            "屏蔽管理 · 崩溃记录 · 诊断 · 关于",
            p,
            vec![
                block_row.into_any_element(),
                crash_row.into_any_element(),
                diag_row.into_any_element(),
                setting_row(
                    "关于",
                    format!("知乎 Rust · M5 · {}", env!("CARGO_PKG_VERSION")),
                )
                .into_any_element(),
            ],
            cx,
        );

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
            .child(account_group)
            .child(content_group)
            .child(recommend_group)
            .child(reading_group)
            .child(tools_group)
            .into_any_element()
    }

    /// One collapsible 我的 section: header row plus rows shown only when open.
    fn profile_group(
        &mut self,
        index: usize,
        title: &'static str,
        hint: &'static str,
        p: &Palette,
        body: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.profile_expanded.get(index).copied().unwrap_or(false);
        let mut card = div()
            .w_full()
            .flex_shrink_0()
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .rounded_lg()
            .overflow_hidden()
            .child(
                div()
                    .id(("profile-group", index))
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.profile_expanded.get_mut(index) {
                            *open = !*open;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.fg)
                                    .child(title),
                            )
                            .child(div().text_xs().text_color(p.muted).child(hint)),
                    )
                    .child(
                        Icon::new(if expanded {
                            gpui_kit::assets::IconName::ChevronUp
                        } else {
                            gpui_kit::assets::IconName::ChevronDown
                        })
                        .size(px(16.))
                        .text_color(p.muted),
                    ),
            );
        if expanded {
            for row in body {
                card = card.child(row);
            }
        }
        card.into_any_element()
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
            .child({
                #[cfg(target_os = "android")]
                {
                    div()
                        .id("web-login")
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(p.elevated)
                        .text_sm()
                        .text_color(p.accent)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_web_login(cx);
                        }))
                        .child("浏览器登录（自动导入）")
                }
                #[cfg(not(target_os = "android"))]
                {
                    div()
                }
            })
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
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ = &this;
                                        #[cfg(target_os = "android")]
                                        {
                                            let pairs = this
                                                .qr_risk_cookies
                                                .iter()
                                                .map(|(name, value)| format!("{name}={value}"))
                                                .collect::<Vec<_>>()
                                                .join("\u{1}");
                                            if let Err(error) =
                                                crate::android_login::open_risk_control(
                                                    &target, &pairs,
                                                )
                                            {
                                                this.login_status =
                                                    Some(format!("打开验证页失败：{error}"));
                                            }
                                        }
                                        #[cfg(not(target_os = "android"))]
                                        {
                                            open_external(&target);
                                        }
                                        cx.notify();
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
                                        this.qr_status =
                                            Some("正在用验证后的会话重新获取二维码…".to_string());
                                        this.services.start_qr_login_with_cookies(
                                            this.qr_risk_cookies.clone(),
                                        );
                                        cx.notify();
                                    }))
                                    .child("完成验证后继续扫码"),
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

    /// Open the standalone sign-in page.
    pub(crate) fn open_login(&mut self, cx: &mut Context<Self>) {
        self.login_open = false;
        self.blocklist_open = false;
        if !matches!(self.stack.last(), Some(super::Route::Login)) {
            self.stack.push(super::Route::Login);
        }
        cx.notify();
    }

    pub(crate) fn render_login_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(self.dark);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let logged_in = self.is_logged_in();
        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(p.border)
            .bg(p.surface)
            .child(
                div()
                    .id("login-back")
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(p.accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_base()
                    .font_weight(FontWeight::BOLD)
                    .text_color(p.fg)
                    .child("登录"),
            )
            .child(div().text_xs().text_color(p.muted).child(if logged_in {
                "已登录"
            } else {
                "未登录"
            }));

        let form = self.render_login_form(cx, &p);
        let _ = window;
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
            .child(header)
            .child(
                div()
                    .id("login-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(form),
            )
            .into_any_element()
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
