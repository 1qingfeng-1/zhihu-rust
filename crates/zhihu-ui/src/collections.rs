use super::*;

impl ZhihuApp {
    fn collection_header(&self, title: &str, cx: &mut Context<Self>) -> Div {
        let surface = if self.dark {
            rgb(0x181a20)
        } else {
            rgb(0xffffff)
        };
        let border = if self.dark {
            rgb(0x2b3139)
        } else {
            rgb(0xeaecef)
        };
        div()
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
                    .id(SharedString::from(format!("collection-back-{title}")))
                    .px_2()
                    .py_1()
                    .text_xl()
                    .text_color(rgb(0xf0b90b))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.back();
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::ArrowLeft)
                            .size(px(20.))
                            .text_color(rgb(0xf0b90b)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(title.to_string())),
            )
    }

    pub(super) fn render_collections(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let bg = if self.dark {
            rgb(0x0b0e11)
        } else {
            rgb(0xfafafa)
        };
        let surface = if self.dark {
            rgb(0x181a20)
        } else {
            rgb(0xffffff)
        };
        let fg = if self.dark {
            rgb(0xeaecef)
        } else {
            rgb(0x1e2329)
        };
        let muted = if self.dark {
            rgb(0x848e9c)
        } else {
            rgb(0x6b7280)
        };
        let border = if self.dark {
            rgb(0x2b3139)
        } else {
            rgb(0xeaecef)
        };
        let accent = rgb(0xf0b90b);
        // Edge-to-edge Android: without the status-bar inset the header (and
        // its back button) is drawn under the system clock and the tap never
        // reaches us.
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let weak = cx.entity().downgrade();
        let cards: Vec<AnyElement> = self
            .collections
            .iter()
            .map(|collection| {
                let weak = weak.clone();
                let id = collection.id.clone();
                let title = collection.title.clone();
                div()
                    .id(SharedString::from(format!("collection-{}", collection.id)))
                    .w_full()
                    .flex_shrink_0()
                    .p_4()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        weak.update(cx, |this, cx| {
                            this.open_collection(id.clone(), title.clone());
                            cx.notify();
                        })
                        .ok();
                    })
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(fg)
                            .child(collection.title.clone()),
                    )
                    .child(
                        div()
                            .pt_1()
                            .text_sm()
                            .text_color(muted)
                            .line_clamp(2)
                            .child(collection.description.clone()),
                    )
                    .child(div().pt_2().text_xs().text_color(accent).child(format!(
                        "{} 篇内容 · {}",
                        collection.item_count,
                        if collection.is_public {
                            "公开"
                        } else {
                            "私密"
                        }
                    )))
                    .into_any_element()
            })
            .collect();
        let content: AnyElement = if self.collections_loading && self.collections.is_empty() {
            empty_state("加载中…", muted).into_any_element()
        } else if let Some(error) = &self.collections_error {
            div()
                .size_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xdc2626))
                        .child(error.clone()),
                )
                .into_any_element()
        } else if self.collections.is_empty() {
            empty_state("暂无收藏夹", muted).into_any_element()
        } else {
            div()
                .id("collections-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .children(cards)
                .child(if self.collections_end {
                    div().id("collections-end")
                } else {
                    div()
                        .id("collections-more")
                        .py_3()
                        .text_sm()
                        .text_color(accent)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.collections_loading {
                                this.collections_loading = true;
                                if let Some(token) = this.account.as_ref().and_then(|account| {
                                    account
                                        .url_token
                                        .clone()
                                        .or_else(|| Some(account.id.clone()))
                                }) {
                                    this.services.load_collections(
                                        token,
                                        this.collections_offset,
                                        true,
                                    );
                                }
                                cx.notify();
                            }
                        }))
                        .child("加载更多")
                })
                .into_any_element()
        };
        div()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .flex()
            .flex_col()
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
            .child(self.collection_header("我的收藏", cx))
            .child(content)
            .into_any_element()
    }

    pub(super) fn render_collection_items(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let title = self
            .stack
            .last()
            .map(|route| route.title().to_string())
            .unwrap_or_else(|| "收藏夹".to_string());
        let bg = if self.dark {
            rgb(0x0b0e11)
        } else {
            rgb(0xfafafa)
        };
        let surface = if self.dark {
            rgb(0x181a20)
        } else {
            rgb(0xffffff)
        };
        let fg = if self.dark {
            rgb(0xeaecef)
        } else {
            rgb(0x1e2329)
        };
        let muted = if self.dark {
            rgb(0x848e9c)
        } else {
            rgb(0x6b7280)
        };
        let border = if self.dark {
            rgb(0x2b3139)
        } else {
            rgb(0xeaecef)
        };
        let accent = rgb(0xf0b90b);
        // Same edge-to-edge inset as the collection list above.
        let (inset_top, inset_bottom, inset_left, inset_right) = safe_area();
        let weak = cx.entity().downgrade();
        let cards: Vec<AnyElement> = self
            .collection_items
            .iter()
            .map(|item| {
                let read = self.is_read(item.kind, &item.id);
                feed_card(
                    item, &weak, surface, fg, muted, border, accent, 0.0, true, read,
                )
            })
            .collect();
        let content: AnyElement =
            if self.collection_items_loading && self.collection_items.is_empty() {
                empty_state("加载中…", muted).into_any_element()
            } else if let Some(error) = &self.collection_items_error {
                div()
                    .size_full()
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xdc2626))
                            .child(error.clone()),
                    )
                    .into_any_element()
            } else if self.collection_items.is_empty() {
                empty_state("收藏夹暂无内容", muted).into_any_element()
            } else {
                div()
                    .id("collection-items-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(cards)
                    .child(if self.collection_items_end {
                        div()
                            .py_2()
                            .text_xs()
                            .text_color(muted)
                            .child("没有更多了")
                            .into_any_element()
                    } else {
                        div()
                            .id("collection-items-more")
                            .py_3()
                            .text_sm()
                            .text_color(accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = match this.stack.last() {
                                    Some(Route::Collection { id, .. }) => id.clone(),
                                    _ => return,
                                };
                                if !this.collection_items_loading {
                                    this.collection_items_loading = true;
                                    this.services.load_collection_items(
                                        id,
                                        this.collection_items_offset,
                                        true,
                                    );
                                    cx.notify();
                                }
                            }))
                            .child(if self.collection_items_loading {
                                "加载中…"
                            } else {
                                "加载更多"
                            })
                            .into_any_element()
                    })
                    .into_any_element()
            };
        div()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .flex()
            .flex_col()
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
            .child(self.collection_header(&title, cx))
            .child(content)
            .into_any_element()
    }

    pub(super) fn render_favorite_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let bg = if self.dark {
            rgb(0x0b0e11)
        } else {
            rgb(0xfafafa)
        };
        let surface = if self.dark {
            rgb(0x181a20)
        } else {
            rgb(0xffffff)
        };
        let fg = if self.dark {
            rgb(0xeaecef)
        } else {
            rgb(0x1e2329)
        };
        let muted = if self.dark {
            rgb(0x848e9c)
        } else {
            rgb(0x6b7280)
        };
        let border = if self.dark {
            rgb(0x2b3139)
        } else {
            rgb(0xeaecef)
        };
        let accent = rgb(0xf0b90b);
        let cards: Vec<AnyElement> = self
            .content_collections
            .iter()
            .map(|collection| {
                let id = collection.id.clone();
                let favorite = !collection.is_favorited;
                div()
                    .id(SharedString::from(format!(
                        "favorite-collection-{}",
                        collection.id
                    )))
                    .w_full()
                    .flex_shrink_0()
                    .p_4()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.favorite_busy {
                            return;
                        }
                        let Some((kind, content_id)) = this
                            .detail
                            .as_ref()
                            .map(|detail| (detail.kind, detail.id.clone()))
                        else {
                            return;
                        };
                        this.favorite_busy = true;
                        this.content_collections_error = None;
                        this.services
                            .set_favorite(kind, content_id, id.clone(), favorite);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(fg)
                                    .child(collection.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if collection.is_favorited {
                                        accent
                                    } else {
                                        muted
                                    })
                                    .child(if collection.is_favorited {
                                        "✓ 已收藏 · 点击取消"
                                    } else {
                                        "＋ 收藏到这里"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .pt_1()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} 篇内容", collection.item_count)),
                    )
                    .into_any_element()
            })
            .collect();
        let content: AnyElement = if self.content_collections_loading {
            empty_state("加载中…", muted).into_any_element()
        } else if self.content_collections.is_empty() && self.content_collections_error.is_none() {
            empty_state("暂无可用收藏夹", muted).into_any_element()
        } else {
            div()
                .id("favorite-collections-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .children(cards)
                .children(self.content_collections_error.as_ref().map(|error| {
                    div()
                        .text_sm()
                        .text_color(rgb(0xdc2626))
                        .child(error.clone())
                }))
                .into_any_element()
        };
        div()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .flex()
            .flex_col()
            .pt(px(safe_area().0))
            .pb(px(safe_area().1))
            .pl(px(safe_area().2))
            .pr(px(safe_area().3))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.back() {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(self.collection_header("选择收藏夹", cx))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .child("点击收藏夹添加或移除当前内容"),
            )
            .child(
                div()
                    .px_4()
                    .pb_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.collection_title_input)
                            .flex_1()
                            .h(px(38.))
                            .bg(surface)
                            .text_color(fg),
                    )
                    .child(
                        div()
                            .id("collection-create")
                            .flex_shrink_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(accent)
                            .text_sm()
                            .text_color(rgb(0x0b0e11))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let title = this
                                    .collection_title_input
                                    .read(cx)
                                    .value()
                                    .trim()
                                    .to_string();
                                if title.is_empty() {
                                    this.content_collections_error =
                                        Some("请输入收藏夹名称".to_string());
                                } else if !this.favorite_busy {
                                    this.favorite_busy = true;
                                    this.content_collections_error = None;
                                    this.services.create_collection(title, String::new(), false);
                                }
                                cx.notify();
                            }))
                            .child("新建收藏夹"),
                    ),
            )
            .child(content)
            .into_any_element()
    }
}
