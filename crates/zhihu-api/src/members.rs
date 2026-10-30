//! Member content and social-list mapping.

use serde_json::Value;
use zhihu_types::{Collection, ContentKind, FeedItem, MemberColumn, MemberProfile};

use crate::feed::{author_of, clean_text};

fn text_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn number_at(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|number| {
                number
                    .as_u64()
                    .or_else(|| number.as_str().and_then(|text| text.parse().ok()))
            })
        })
        .unwrap_or(0)
}

fn id_of(value: &Value) -> Option<String> {
    value.get("id").and_then(|id| {
        id.as_i64()
            .map(|number| number.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    })
}

fn page_end(value: &Value, len: usize) -> bool {
    value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(len < 20)
}

fn collection_page(value: &Value) -> (Vec<Collection>, bool) {
    let items = crate::collections::parse_collections(value);
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    (items, is_end)
}

pub fn parse_member_favlists(value: &Value) -> (Vec<Collection>, bool) {
    collection_page(value)
}

pub fn parse_member_people(value: &Value) -> (Vec<MemberProfile>, bool) {
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(crate::detail::parse_member)
        .collect::<Vec<_>>();
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    (items, is_end)
}

/// Pin moments sometimes return bare pin objects without a `type` discriminator.
/// Normalize those before handing them to the shared feed mapper.
pub fn parse_member_pins(value: &Value) -> (Vec<FeedItem>, bool) {
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let target = entry.get("target").unwrap_or(entry);
            if target.get("type").and_then(Value::as_str).is_some() {
                return crate::feed::parse_target(target);
            }
            if target.get("content").is_some() && target.get("id").is_some() {
                let mut normalized = target.clone();
                normalized["type"] = Value::String("pin".to_string());
                return crate::feed::parse_target(&normalized);
            }
            None
        })
        .collect::<Vec<_>>();
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    (items, is_end)
}

/// Questions asked by a member (`members/{token}/questions`).
///
/// Follower count maps to `vote_up_count` and answer count to `comment_count`
/// so the shared member card can render "关注 · 回答" without a new type.
pub fn parse_member_questions(value: &Value) -> (Vec<FeedItem>, bool) {
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let question = entry
                .get("target")
                .filter(|target| target.is_object())
                .unwrap_or(entry);
            let id = id_of(question)?;
            let title = text_at(question, &["title"])?;
            let excerpt = text_at(question, &["detail", "description", "excerpt"])
                .map(|text| clean_text(&text))
                .unwrap_or_default();
            Some(FeedItem {
                id,
                title: clean_text(&title),
                excerpt,
                author: author_of(question),
                kind: ContentKind::Question,
                vote_up_count: number_at(question, &["follower_count", "followerCount"]),
                comment_count: number_at(question, &["answer_count", "answerCount"]),
                created_at: number_at(question, &["created", "created_time", "createdTime"]) as i64,
                topics: Vec::new(),
                topic_refs: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    let is_end = page_end(value, items.len());
    (items, is_end)
}

/// Columns a member contributes to (`members/{token}/column-contributions`).
pub fn parse_member_columns(value: &Value) -> (Vec<MemberColumn>, bool) {
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let column = entry
                .get("column")
                .filter(|column| column.is_object())
                .unwrap_or(entry);
            let id = id_of(column)?;
            Some(MemberColumn {
                id,
                title: text_at(column, &["title"])
                    .map(|title| clean_text(&title))
                    .unwrap_or_else(|| "未命名专栏".to_string()),
                description: text_at(column, &["description", "intro", "excerpt"])
                    .map(|text| clean_text(&text))
                    .unwrap_or_default(),
                article_count: number_at(column, &["articles_count", "article_count"]),
                follower_count: number_at(
                    column,
                    &["followers_count", "follower_count", "followers"],
                ),
            })
        })
        .collect::<Vec<_>>();
    let is_end = page_end(value, items.len());
    (items, is_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zhihu_types::ContentKind;

    #[test]
    fn parses_bare_pin_moments() {
        let value = json!({
            "data": [{
                "id": 55,
                "excerpt_title": "想法标题",
                "content": [{ "type": "text", "content": "正文" }],
                "like_count": 4,
                "comment_count": 2,
                "author": { "name": "alice" }
            }]
        });
        let (items, is_end) = parse_member_pins(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ContentKind::Pin);
        assert_eq!(items[0].id, "55");
        assert_eq!(items[0].excerpt, "想法标题");
        assert!(is_end);
    }

    #[test]
    fn parses_member_favlists() {
        let value = json!({
            "data": [{
                "id": "9",
                "title": "阅读清单",
                "item_count": 6,
                "is_public": true
            }],
            "paging": { "is_end": false }
        });
        let (items, is_end) = parse_member_favlists(&value);
        assert_eq!(items[0].title, "阅读清单");
        assert_eq!(items[0].item_count, 6);
        assert!(!is_end);
    }

    #[test]
    fn parses_follower_people() {
        let value = json!({
            "data": [{
                "id": "u1",
                "name": "alice",
                "url_token": "alice",
                "headline": "writer",
                "follower_count": 12,
                "is_following": true
            }],
            "paging": { "is_end": true }
        });
        let (items, is_end) = parse_member_people(&value);
        assert_eq!(items[0].url_token, "alice");
        assert_eq!(items[0].follower_count, 12);
        assert!(items[0].is_following);
        assert!(is_end);
    }

    #[test]
    fn parses_member_questions() {
        let value = json!({
            "data": [{
                "id": 7788,
                "type": "question",
                "title": "如何评价 <em>Rust</em>？",
                "detail": "<p>想听听大家的看法</p>",
                "answer_count": 42,
                "follower_count": 512,
                "author": { "name": "alice", "url_token": "alice" }
            }],
            "paging": { "is_end": true }
        });
        let (items, is_end) = parse_member_questions(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "7788");
        assert_eq!(items[0].kind, ContentKind::Question);
        assert_eq!(items[0].title, "如何评价 Rust？");
        assert_eq!(items[0].excerpt, "想听听大家的看法");
        assert_eq!(items[0].vote_up_count, 512);
        assert_eq!(items[0].comment_count, 42);
        assert_eq!(items[0].author.name, "alice");
        assert!(is_end);
    }

    #[test]
    fn skips_questions_without_title() {
        let value = json!({ "data": [{ "id": 1, "answer_count": 3 }] });
        let (items, is_end) = parse_member_questions(&value);
        assert!(items.is_empty());
        assert!(is_end);
    }

    #[test]
    fn parses_member_columns() {
        let value = json!({
            "data": [{
                "id": "column-1",
                "title": "Rust 手记",
                "description": "记录 Rust 实战",
                "articles_count": 18,
                "followers": 900,
                "type": "column"
            }, {
                "column": {
                    "id": 42,
                    "title": "嵌套专栏",
                    "article_count": 3
                }
            }],
            "paging": { "is_end": false }
        });
        let (items, is_end) = parse_member_columns(&value);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "column-1");
        assert_eq!(items[0].article_count, 18);
        assert_eq!(items[0].follower_count, 900);
        assert_eq!(items[1].id, "42");
        assert_eq!(items[1].title, "嵌套专栏");
        assert!(!is_end);
    }
}
