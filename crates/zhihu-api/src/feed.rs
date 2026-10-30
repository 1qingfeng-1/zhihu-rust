//! Mapping from the polymorphic Zhihu feed JSON to domain [`FeedItem`]s.

use serde_json::Value;
use zhihu_types::{Author, ContentKind, FeedItem};

fn string_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn number_at(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn contains_mcn(value: &Value) -> bool {
    match value {
        Value::String(text) => text.to_ascii_lowercase().contains("mcn"),
        Value::Array(items) => items.iter().any(contains_mcn),
        Value::Object(map) => map
            .iter()
            .any(|(key, item)| key.to_ascii_lowercase().contains("mcn") || contains_mcn(item)),
        _ => false,
    }
}

pub(crate) fn is_mcn_author(value: &Value) -> bool {
    value
        .get("is_mcn")
        .or_else(|| value.get("isMcn"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value.get("mcn_info").is_some_and(|info| !info.is_null())
        || ["badge_v2", "badgeV2", "badge"]
            .iter()
            .filter_map(|key| value.get(*key))
            .any(contains_mcn)
}

pub(crate) fn author_of(value: &Value) -> Author {
    let author = value.get("author");
    let author_value = author.unwrap_or(&Value::Null);
    Author {
        id: author
            .and_then(|author| author.get("id"))
            .and_then(|id| {
                id.as_str()
                    .map(str::to_string)
                    .or_else(|| id.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default(),
        name: author
            .and_then(|author| string_at(author, &["name"]))
            .unwrap_or("匿名用户")
            .to_string(),
        headline: author
            .and_then(|author| string_at(author, &["headline", "description"]))
            .map(str::to_string),
        url_token: author
            .and_then(|author| string_at(author, &["url_token"]))
            .map(str::to_string),
        is_following: author
            .and_then(|author| author.get("is_following"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        avatar_url: author
            .and_then(|author| string_at(author, &["avatar_url", "avatarUrl"]))
            .or_else(|| {
                author.and_then(|author| {
                    string_at(author, &["avatar_url_template", "avatarUrlTemplate"])
                })
            })
            .map(|url| url.replace("{size}", "xl")),
        is_mcn: is_mcn_author(author_value),
    }
}

fn topics_of(target: &Value) -> Vec<String> {
    topic_refs_of(target)
        .into_iter()
        .map(|topic| topic.name)
        .collect()
}

fn topic_refs_of(target: &Value) -> Vec<zhihu_types::TopicRef> {
    target
        .get("topics")
        .or_else(|| target.get("question").and_then(|q| q.get("topics")))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|topic| {
            let name = topic.get("name").and_then(Value::as_str)?;
            let id = topic
                .get("id")
                .and_then(|id| {
                    id.as_i64()
                        .map(|n| n.to_string())
                        .or_else(|| id.as_str().map(str::to_string))
                })
                .unwrap_or_default();
            Some(zhihu_types::TopicRef {
                id,
                name: name.to_string(),
            })
        })
        .collect()
}

fn target_type(target: &Value) -> Option<&str> {
    string_at(target, &["type"])
}

/// Strip search highlight tags and decode the entities Zhihu emits in
/// excerpts. Search results wrap matches in `<em>...</em>` and may include
/// `<br>` between paragraphs.
pub(crate) fn clean_text(value: &str) -> String {
    let value = value
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .replace("</p>", " ");
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean_opt(value: Option<&str>) -> String {
    value.map(clean_text).unwrap_or_default()
}

/// Parse `/api/v3/feed/topstory/recommend` (and friends) into cards.
pub fn parse_feed(value: &Value) -> Vec<FeedItem> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();

    entries.iter().filter_map(parse_entry).collect()
}

/// One recommendation page plus the API's end marker. Falls back to a
/// short-page heuristic when `paging` is absent.
pub fn parse_feed_page(value: &Value) -> (Vec<FeedItem>, bool) {
    let items = parse_feed(value);
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 10);
    (items, is_end)
}

/// Next-page URL for APIs that use a server-provided cursor.
pub fn parse_feed_next(value: &Value) -> Option<String> {
    value
        .get("paging")
        .and_then(|paging| paging.get("next"))
        .and_then(Value::as_str)
        .filter(|next| !next.is_empty())
        .map(str::to_string)
}

/// Parse a question answer page. The `data` array contains answer objects
/// directly (unlike the feed's `target` wrappers).
pub fn parse_answers_page(value: &Value) -> (Vec<FeedItem>, bool) {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let items: Vec<FeedItem> = entries.iter().filter_map(parse_target).collect();
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    (items, is_end)
}

fn parse_entry(entry: &Value) -> Option<FeedItem> {
    let target = entry.get("target")?;
    let mut item = parse_target(target)?;
    if item.id.is_empty() {
        item.id = string_at(entry, &["id"]).unwrap_or_default().to_string();
    }
    Some(item)
}

/// Parse a single feed/search target object.
pub fn parse_target(target: &Value) -> Option<FeedItem> {
    let kind = match target_type(target)? {
        "answer" => ContentKind::Answer,
        "article" => ContentKind::Article,
        "pin" => ContentKind::Pin,
        "question" => ContentKind::Question,
        "zvideo" => ContentKind::Question,
        _ => return None,
    };

    let id = target.get("id").and_then(|id| {
        id.as_i64()
            .map(|n| n.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    })?;

    let (title, excerpt) = match kind {
        ContentKind::Answer => {
            let title = target
                .get("question")
                .and_then(|question| string_at(question, &["title"]))
                .map(clean_text)
                .or_else(|| {
                    string_at(target, &["title"])
                        .map(clean_text)
                        .filter(|title| !title.is_empty())
                })
                .unwrap_or_else(|| "回答".to_string());
            (title, clean_opt(string_at(target, &["excerpt", "content"])))
        }
        ContentKind::Article => (
            string_at(target, &["title"])
                .map(clean_text)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "文章".to_string()),
            clean_opt(string_at(target, &["excerpt", "content"])),
        ),
        ContentKind::Pin => {
            let excerpt = clean_opt(string_at(
                target,
                &["excerpt_title", "content_html", "content"],
            ));
            (
                if excerpt.is_empty() {
                    "想法".to_string()
                } else {
                    excerpt.chars().take(40).collect()
                },
                excerpt,
            )
        }
        ContentKind::Question => (
            string_at(target, &["title", "description"])
                .map(clean_text)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "视频".to_string()),
            clean_opt(string_at(target, &["excerpt", "description"])),
        ),
    };

    Some(FeedItem {
        id,
        title,
        excerpt,
        author: author_of(target),
        kind,
        vote_up_count: number_at(
            target,
            &["voteup_count", "voteupCount", "like_count", "vote_count"],
        ),
        comment_count: number_at(target, &["comment_count", "commentCount"]),
        created_at: number_at(
            target,
            &[
                "created_time",
                "created",
                "createdTime",
                "updated_time",
                "updated",
            ],
        ) as i64,
        topics: topics_of(target),
        topic_refs: topic_refs_of(target),
    })
}

/// Parse  results ().
/// One page of `search_v3` results.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchPage {
    pub items: Vec<FeedItem>,
    pub is_end: bool,
}

pub fn parse_search_page(value: &Value) -> SearchPage {
    let items = parse_search(value);
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    SearchPage { items, is_end }
}

pub fn parse_search(value: &Value) -> Vec<FeedItem> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let object = entry.get("object").unwrap_or(entry);
            parse_target(object)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_feed_paging_end() {
        let more = json!({ "data": [], "paging": { "is_end": false } });
        let (items, is_end) = parse_feed_page(&more);
        assert!(items.is_empty());
        assert!(!is_end);

        let last = json!({ "data": [], "paging": { "is_end": true } });
        assert!(parse_feed_page(&last).1);

        // No paging object: short pages are treated as the end.
        assert!(parse_feed_page(&json!({ "data": [] })).1);
    }

    #[test]
    fn detects_explicit_mcn_authors() {
        assert!(is_mcn_author(&json!({ "is_mcn": true })));
        assert!(is_mcn_author(&json!({
            "badge_v2": { "detail_badges": [{ "title": "所属 MCN 机构" }] }
        })));
        assert!(!is_mcn_author(&json!({ "name": "普通用户" })));
    }

    #[test]
    fn parses_feed_next_cursor() {
        let value = json!({
            "data": [],
            "paging": { "is_end": false, "next": "https://api.zhihu.com/moments_v3?offset=10" }
        });
        assert_eq!(
            parse_feed_next(&value).as_deref(),
            Some("https://api.zhihu.com/moments_v3?offset=10")
        );
        assert_eq!(parse_feed_next(&json!({ "data": [] })), None);
    }

    #[test]
    fn parses_question_answer_page() {
        let value = json!({
            "data": [
                {
                    "type": "answer",
                    "id": 7,
                    "excerpt": "hi",
                    "voteup_count": 3,
                    "comment_count": 1,
                    "author": { "name": "alice" },
                    "question": { "id": 99, "title": "Q" }
                }
            ],
            "paging": { "is_end": false }
        });
        let (items, is_end) = parse_answers_page(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ContentKind::Answer);
        assert_eq!(items[0].id, "7");
        assert!(!is_end);
    }

    #[test]
    fn parses_answer_entry() {
        let value = json!({
            "data": [{
                "type": "feed",
                "target": {
                    "type": "answer",
                    "id": 42,
                    "excerpt": "hello",
                    "voteup_count": 7,
                    "comment_count": 3,
                    "author": { "name": "alice", "headline": "writer" },
                    "question": { "title": "why?" }
                }
            }]
        });
        let items = parse_feed(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "42");
        assert_eq!(items[0].title, "why?");
        assert_eq!(items[0].kind, ContentKind::Answer);
        assert_eq!(items[0].author.name, "alice");
        assert_eq!(items[0].vote_up_count, 7);
    }

    #[test]
    fn parses_search_result_objects() {
        let value = json!({
            "data": [{
                "type": "search_result",
                "object": {
                    "type": "answer",
                    "id": 11,
                    "excerpt": "body",
                    "voteup_count": 9,
                    "comment_count": 2,
                    "author": { "name": "alice" },
                    "question": { "title": "question" }
                }
            }]
        });
        let items = parse_search(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "11");
        assert_eq!(items[0].title, "question");
        assert_eq!(items[0].vote_up_count, 9);
    }

    #[test]
    fn search_uses_object_title_and_strips_highlight_tags() {
        let value = json!({
            "data": [{
                "type": "search_result",
                "object": {
                    "type": "answer",
                    "id": 77,
                    "title": "怎样看待 Rust？",
                    "excerpt": "作为技术挑战，<em>rust</em> 挺有趣<br>但项目成本高",
                    "author": { "name": "alice" }
                }
            }]
        });
        let items = parse_search(&value);
        assert_eq!(items[0].title, "怎样看待 Rust？");
        assert_eq!(items[0].excerpt, "作为技术挑战，rust 挺有趣 但项目成本高");
        assert!(!items[0].excerpt.contains('<'));
    }

    #[test]
    fn parses_search_paging() {
        let value = json!({
            "data": [{
                "type": "search_result",
                "object": {
                    "type": "answer",
                    "id": 11,
                    "author": { "name": "alice" },
                    "question": { "title": "question" }
                }
            }],
            "paging": { "is_end": true }
        });
        let page = parse_search_page(&value);
        assert_eq!(page.items.len(), 1);
        assert!(page.is_end);
    }

    #[test]
    fn skips_ads_without_target() {
        let value = json!({ "data": [{ "type": "feed", "brief": "ad" }] });
        assert!(parse_feed(&value).is_empty());
    }
}
