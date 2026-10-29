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

/// One page of the notification timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationsPage {
    pub items: Vec<NotificationEntry>,
    pub is_end: bool,
}

pub fn parse_notifications_page(value: &Value) -> NotificationsPage {
    let items = parse_notifications(value);
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    NotificationsPage { items, is_end }
}

pub fn parse_notifications(value: &Value) -> Vec<NotificationEntry> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = text_at(item, "uniqueId")
                .or_else(|| text_at(item, "id"))
                .or_else(|| text_at(item, "detailTitle"))?;
            let content = item.get("content").filter(|value| value.is_object());
            let head = item.get("head").filter(|value| value.is_object());
            let target = item.get("target").filter(|value| value.is_object());

            let title = content
                .and_then(|content| text_at(content, "title"))
                .or_else(|| text_at(item, "detailTitle"))
                .unwrap_or_else(|| "通知".to_string());
            let text = content
                .and_then(|content| {
                    text_at(content, "text").or_else(|| text_at(content, "abstractText"))
                })
                .or_else(|| text_at(item, "detailTitle"))
                .unwrap_or_default();
            let actor_author = head.and_then(|head| head.get("author")).or(target);
            let actor = actor_author
                .and_then(|author| text_at(author, "name"))
                .unwrap_or_default();
            let actor_token = actor_author
                .and_then(|author| {
                    text_at(author, "url_token").or_else(|| {
                        author.get("id").and_then(|id| {
                            id.as_str()
                                .map(str::to_string)
                                .or_else(|| id.as_i64().map(|id| id.to_string()))
                        })
                    })
                })
                .filter(|token| !token.is_empty());
            let actor_avatar = actor_author.and_then(|author| text_at(author, "avatar_url"));
            let created = item
                .get("created")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let is_read = item.get("isRead").and_then(Value::as_bool).unwrap_or(true);

            Some(NotificationEntry {
                id,
                title,
                text,
                actor,
                actor_token,
                actor_avatar,
                created,
                is_read,
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
