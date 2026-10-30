//! Collection and favorite API mapping.

use serde_json::Value;
use zhihu_types::{Collection, FeedItem};

fn string_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn id_of(value: &Value) -> Option<String> {
    value.get("id").and_then(|id| {
        id.as_i64()
            .map(|n| n.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    })
}

pub fn parse_collection(value: &Value) -> Option<Collection> {
    Some(Collection {
        id: id_of(value)?,
        title: string_at(value, &["title"]).unwrap_or("收藏夹").to_string(),
        description: string_at(value, &["description"])
            .unwrap_or_default()
            .to_string(),
        item_count: value
            .get("item_count")
            .or_else(|| value.get("answer_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        is_public: value
            .get("is_public")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_default: value
            .get("is_default")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_favorited: value
            .get("is_favorited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn parse_collections(value: &Value) -> Vec<Collection> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_collection)
        .collect()
}

/// A collection wraps its actual answer/article in `content`, unlike a feed's
/// `target` wrapper.
pub fn parse_collection_items(value: &Value) -> (Vec<FeedItem>, bool) {
    let items: Vec<_> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("content"))
        .filter_map(crate::feed::parse_target)
        .collect();
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    (items, is_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zhihu_types::ContentKind;

    #[test]
    fn parses_collection_list_and_detail_envelope() {
        let value = json!({
            "data": [{
                "id": 42,
                "title": "Rust",
                "description": "阅读清单",
                "item_count": 3,
                "is_public": true,
                "is_default": true,
                "is_favorited": true
            }]
        });
        let items = parse_collections(&value);
        assert_eq!(items[0].id, "42");
        assert_eq!(items[0].item_count, 3);
        assert!(items[0].is_favorited);
        assert_eq!(
            parse_collection(&json!({ "collection": value["data"][0].clone() })).map(|_| ()),
            None
        );
    }

    #[test]
    fn unwraps_collection_content_and_paging() {
        let value = json!({
            "data": [{ "created": "1", "content": {
                "type": "answer", "id": 7, "excerpt": "hello",
                "author": { "name": "alice" },
                "question": { "title": "Question" }
            }}],
            "paging": { "is_end": false }
        });
        let (items, is_end) = parse_collection_items(&value);
        assert_eq!(items[0].kind, ContentKind::Answer);
        assert_eq!(items[0].id, "7");
        assert!(!is_end);
    }
}
