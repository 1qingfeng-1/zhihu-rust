use super::*;

impl ZhihuApp {
    pub(super) fn render_topic(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let bg = if dark { rgb(0x0b0e11) } else { rgb(0xfafafa) };
        let surface = if dark { rgb(0x181a20) } else { rgb(0xffffff) };
        let fg = if dark { rgb(0xeaecef) } else { rgb(0x1e2329) };
        let muted = if dark { rgb(0x848e9c) } else { rgb(0x6b7280) };
        let border = if dark { rgb(0x2b3139) } else { rgb(0xeaecef) };
        let accent = rgb(0xf0b90b);
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let title = self
            .stack
            .last()
            .map(|route| route.title().to_string())
            .unwrap_or_else(|| "话题".to_string());
        let following = self.topic_following;

        let header = div()
            .h(px(56.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .id("topic-back")
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("topic-follow")
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(if following { surface } else { accent })
                    .border_1()
                    .border_color(if following { border } else { accent })
                    .text_sm()
                    .text_color(if following { muted } else { rgb(0x0b0e11) })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_topic_follow(cx);
                    }))
                    .child(if following { "已关注" } else { "+ 关注" }),
            );

        let mut column = div()
            .id("topic-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.topic_scroll)
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3();

        if self.topic_loading && self.topic.is_none() {
            column = column.child(empty_state("话题加载中…", muted));
        }
        if let Some(error) = &self.topic_error {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xf6465d))
                    .child(SharedString::from(error.clone())),
            );
        }
        if let Some(topic) = &self.topic {
            let avatar = topic
                .avatar_url
                .as_ref()
                .map(|url| {
                    img(url.clone())
                        .size(px(52.))
                        .rounded_full()
                        .into_any_element()
                })
                .unwrap_or_else(|| {
                    div()
                        .size(px(52.))
                        .rounded_full()
                        .bg(accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x0b0e11))
                                .child(SharedString::from(
                                    topic.name.chars().next().unwrap_or('话').to_string(),
                                )),
                        )
                        .into_any_element()
                });
            let mut card = div()
                .w_full()
                .flex_shrink_0()
                .p_4()
                .rounded_lg()
                .bg(surface)
                .border_1()
                .border_color(border)
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div().flex().items_center().gap_3().child(avatar).child(
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
                                    .text_color(fg)
                                    .child(SharedString::from(topic.name.clone())),
                            )
                            .child(div().text_xs().text_color(muted).child(format!(
                                "{} 关注 · {} 问题 · {} 讨论",
                                topic.followers_count, topic.questions_count, topic.discuss_count
                            ))),
                    ),
                );
            if !topic.excerpt.trim().is_empty() {
                card = card.child(
                    div()
                        .w_full()
                        .text_sm()
                        .text_color(muted)
                        .child(SharedString::from(topic.excerpt.clone())),
                );
            }
            column = column.child(card);
        }

        let mut tabs = div()
            .id("topic-tabs")
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2();
        for tab in [
            TopicFeedTab::Discussion,
            TopicFeedTab::Ideas,
            TopicFeedTab::Unanswered,
        ] {
            let active = self.topic_tab == tab;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("topic-tab-{}", tab.label())))
                    .flex_1()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .bg(if active { accent } else { surface })
                    .border_1()
                    .border_color(if active { accent } else { border })
                    .text_sm()
                    .text_color(if active { rgb(0x0b0e11) } else { fg })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_topic_tab(tab, cx);
                    }))
                    .child(tab.label()),
            );
        }
        column = column.child(tabs);

        if self.topic_tab != TopicFeedTab::Unanswered {
            let sorts: &[TopicSort] = if self.topic_tab == TopicFeedTab::Ideas {
                &[TopicSort::Hot, TopicSort::Latest]
            } else {
                &[TopicSort::Essence, TopicSort::Hot, TopicSort::Latest]
            };
            let mut sort_row = div()
                .id("topic-sorts")
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2();
            for sort in sorts {
                let active = self.topic_sort == *sort;
                sort_row = sort_row.child(
                    div()
                        .id(SharedString::from(format!("topic-sort-{}", sort.label())))
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .bg(if active { surface } else { bg })
                        .border_1()
                        .border_color(if active { accent } else { border })
                        .text_xs()
                        .text_color(if active { accent } else { muted })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_topic_sort(*sort, cx);
                        }))
                        .child(sort.label()),
                );
            }
            column = column.child(sort_row);
        }

        if let Some(error) = &self.topic_feed_error {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xf6465d))
                    .child(SharedString::from(error.clone())),
            );
        }
        if self.topic_items.is_empty() {
            if !self.topic_loading_more && self.topic_feed_error.is_none() {
                column = column.child(empty_state("该分类暂无内容", muted));
            }
        } else {
            let weak = cx.entity().downgrade();
            for item in &self.topic_items {
                column = column.child(topic_item_card(
                    item, &weak, surface, fg, muted, border, accent,
                ));
            }
            if self.topic_end {
                column = column.child(div().py_2().text_xs().text_color(muted).child("没有更多了"));
            } else {
                column = column.child(
                    div()
                        .id("topic-more")
                        .py_3()
                        .text_sm()
                        .text_color(accent)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_more_topic_feed(cx);
                        }))
                        .child(if self.topic_loading_more {
                            "加载中…"
                        } else {
                            "加载更多"
                        }),
                );
            }
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .pt(px(inset_top))
            .pb(px(inset_bottom))
            .pl(px(inset_left))
            .pr(px(inset_right))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(header)
            .child(column)
            .into_any_element()
    }
}

fn topic_item_card(
    item: &FeedItem,
    entity: &WeakEntity<ZhihuApp>,
    surface: Rgba,
    fg: Rgba,
    muted: Rgba,
    border: Rgba,
    accent: Rgba,
) -> AnyElement {
    let kind = item.kind;
    let id = item.id.clone();
    let title = item.title.clone();
    let weak = entity.clone();
    let author_weak = entity.clone();
    let author_token = item
        .author
        .url_token
        .clone()
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| item.author.id.clone());
    let author_name = item.author.name.clone();
    let mut author = div()
        .id(SharedString::from(format!("topic-author-{}", item.id)))
        .text_xs()
        .text_color(muted)
        .child(SharedString::from(item.author.name.clone()));
    if !author_token.is_empty() {
        author = author.cursor_pointer().on_click(move |_, _, cx| {
            author_weak
                .update(cx, |this, cx| {
                    this.open_member(author_token.clone(), author_name.clone());
                    cx.notify();
                })
                .ok();
            cx.stop_propagation();
        });
    }
    div()
        .id(SharedString::from(format!("topic-item-{}", item.id)))
        .w_full()
        .flex_shrink_0()
        .p_4()
        .rounded_lg()
        .bg(surface)
        .border_1()
        .border_color(border)
        .flex()
        .flex_col()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            weak.update(cx, |this, cx| {
                this.open_content(kind, id.clone(), title.clone());
                cx.notify();
            })
            .ok();
        })
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(fg)
                .line_clamp(2)
                .child(SharedString::from(item.title.clone())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(author)
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(accent)
                        .text_xs()
                        .text_color(rgb(0x0b0e11))
                        .child(item.kind.label()),
                )
                .child(div().text_xs().text_color(muted).child(format!(
                    "{} 赞 · {} 评论",
                    item.vote_up_count, item.comment_count
                ))),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .line_clamp(4)
                .child(SharedString::from(item.excerpt.clone())),
        )
        .into_any_element()
}
