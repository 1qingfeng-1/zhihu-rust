//! Notification timeline mapping (`api.zhihu.com/notifications/v3/message/v3`).

use serde_json::Value;
use zhihu_types::NotificationEntry;

fn text_at(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn text_at_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| text_at(value, key))
}

/// One page of the notification timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationsPage {
    pub items: Vec<NotificationEntry>,
    /// Number of timeline entries (excluding the pinned  cards).
    pub timeline_len: usize,
    pub is_end: bool,
    pub total_unread: u64,
}

pub fn parse_notifications_page(value: &Value) -> NotificationsPage {
    let head = parse_head_entries(value);
    let timeline = parse_timeline_entries(value);
    let timeline_len = timeline.len();
    let mut items = head;
    items.extend(timeline);
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end").or_else(|| paging.get("isEnd")))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    let total_unread = value
        .get("unread")
        .and_then(|unread| unread.get("message"))
        .and_then(|message| message.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| items.iter().filter(|item| !item.is_read).count() as u64);
    NotificationsPage {
        items,
        timeline_len,
        is_end,
        total_unread,
    }
}

pub fn parse_notifications(value: &Value) -> Vec<NotificationEntry> {
    let mut items = parse_head_entries(value);
    items.extend(parse_timeline_entries(value));
    items
}

/// Entry cards pinned above the timeline (`head`), such as "邀请回答".
fn parse_head_entries(value: &Value) -> Vec<NotificationEntry> {
    value
        .get("head")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, item)| {
            let content = item.get("content").filter(|value| value.is_object());
            let head = item.get("head").filter(|value| value.is_object());
            let title = content
                .and_then(|content| text_at_any(content, &["title"]))
                .or_else(|| text_at_any(item, &["detailTitle", "detail_title", "title"]))
                .unwrap_or_else(|| "通知".to_string());
            if title.is_empty() {
                return None;
            }
            let text = content
                .and_then(|content| {
                    text_at_any(content, &["text", "abstractText", "abstract_text"])
                })
                .unwrap_or_default();
            let actor_author = head.and_then(|head| head.get("author"));
            let actor = actor_author
                .and_then(|author| text_at_any(author, &["name"]))
                .unwrap_or_default();
            let target_link = content
                .and_then(|content| text_at_any(content, &["targetLink", "target_link"]))
                .or_else(|| {
                    head.and_then(|head| text_at_any(head, &["targetLink", "target_link"]))
                });
            let is_read = item
                .get("isRead")
                .or_else(|| item.get("is_read"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(NotificationEntry {
                id: text_at_any(item, &["id", "noti_subtype"])
                    .map(|id| format!("head-{id}-{index}"))
                    .unwrap_or_else(|| format!("head-{index}")),
                title,
                text,
                actor,
                actor_token: None,
                actor_avatar: None,
                created: item
                    .get("created")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                is_read,
                target_link,
                target_url: None,
            })
        })
        .collect()
}

fn parse_timeline_entries(value: &Value) -> Vec<NotificationEntry> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = text_at_any(item, &["uniqueId", "unique_id"])
                .or_else(|| text_at_any(item, &["id"]))
                .or_else(|| text_at_any(item, &["detailTitle", "detail_title"]))?;
            let content = item.get("content").filter(|value| value.is_object());
            let head = item.get("head").filter(|value| value.is_object());
            let target = item.get("target").filter(|value| value.is_object());
            let target_source = item
                .get("targetSource")
                .or_else(|| item.get("target_source"))
                .filter(|value| value.is_object());

            let title = content
                .and_then(|content| text_at_any(content, &["title"]))
                .or_else(|| text_at_any(item, &["detailTitle", "detail_title"]))
                .unwrap_or_else(|| "通知".to_string());
            let text = content
                .and_then(|content| {
                    text_at_any(content, &["text"])
                        .or_else(|| text_at_any(content, &["abstractText", "abstract_text"]))
                })
                .or_else(|| text_at_any(item, &["detailTitle", "detail_title"]))
                .unwrap_or_default();
            let actor_author = head.and_then(|head| head.get("author")).or(target);
            let actor = actor_author
                .and_then(|author| text_at_any(author, &["name"]))
                .unwrap_or_default();
            let actor_token = actor_author
                .and_then(|author| {
                    text_at_any(author, &["url_token", "urlToken"]).or_else(|| {
                        author.get("id").and_then(|id| {
                            id.as_str()
                                .map(str::to_string)
                                .or_else(|| id.as_i64().map(|id| id.to_string()))
                        })
                    })
                })
                .filter(|token| !token.is_empty());
            let actor_avatar =
                actor_author.and_then(|author| text_at_any(author, &["avatar_url", "avatarUrl"]));
            let created = item
                .get("created")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let is_read = item
                .get("isRead")
                .or_else(|| item.get("is_read"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let target_link =
                content.and_then(|content| text_at_any(content, &["targetLink", "target_link"]));
            let target_url = target_source
                .and_then(|source| text_at_any(source, &["targetLink", "target_link"]));

            Some(NotificationEntry {
                id,
                title,
                text,
                actor,
                actor_token,
                actor_avatar,
                created,
                is_read,
                target_link,
                target_url,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_notification_paging() {
        let value = json!({
            "data": [],
            "paging": { "is_end": true }
        });
        assert!(parse_notifications_page(&value).is_end);
    }

    #[test]
    fn parses_unread_count_and_comment_target() {
        let value = json!({
            "unread": { "message": { "count": 7 } },
            "data": [{
                "unique_id": "n2",
                "is_read": false,
                "created": 1700000000,
                "head": { "author": { "name": "bob", "url_token": "bob" } },
                "content": {
                    "title": "bob 回复了你的评论",
                    "text": "好的",
                    "target_link": "zhihu://comment/list/answer/2?anchor_comment_id=3&is_child=true"
                },
                "target_source": { "target_link": "https://www.zhihu.com/question/1/answer/2" }
            }]
        });
        let page = parse_notifications_page(&value);
        assert_eq!(page.total_unread, 7);
        assert_eq!(page.items[0].id, "n2");
        let anchor = page.items[0].comment_anchor().unwrap();
        assert_eq!(anchor.kind, zhihu_types::ContentKind::Answer);
        assert_eq!(anchor.comment_id, "3");
        assert!(anchor.is_child);
        assert_eq!(
            page.items[0].content_target(),
            Some((zhihu_types::ContentKind::Answer, "2".to_string()))
        );
    }

    #[test]
    fn parses_head_entry_cards() {
        let value = json!({
            "unread": { "message": { "count": 2 } },
            "head": [
                {
                    "id": "entry_invite",
                    "is_read": false,
                    "detail_title": "邀请回答",
                    "content": {
                        "title": "邀请回答",
                        "target_link": "https://www.zhihu.com/notifications/v3/timeline/entry/invite"
                    }
                },
                {
                    "id": "entry_vote",
                    "is_read": false,
                    "detail_title": "赞同和感谢",
                    "content": {
                        "title": "赞同和感谢",
                        "target_link": "https://www.zhihu.com/notifications/v3/timeline/entry/vote"
                    }
                }
            ],
            "data": []
        });
        let page = parse_notifications_page(&value);
        assert_eq!(page.total_unread, 2);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].title, "邀请回答");
        assert!(page.items[0]
            .target_link
            .as_deref()
            .unwrap()
            .contains("invite"));
        assert!(!page.items[1].is_read);
    }

    #[test]
    fn parses_notification_timeline() {
        let value = json!({
            "data": [{
                "id": "n1",
                "uniqueId": "n1",
                "isRead": false,
                "created": 1700000000,
                "detailTitle": "有人赞同了你的回答",
                "head": { "author": { "name": "alice" } },
                "content": { "title": "赞同", "text": "你的回答收到了新的赞同" }
            }]
        });
        let items = parse_notifications(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].actor, "alice");
        assert_eq!(items[0].title, "赞同");
        assert!(!items[0].is_read);
    }
}
