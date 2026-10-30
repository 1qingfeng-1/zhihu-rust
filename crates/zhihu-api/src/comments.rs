//! Comment list mapping (`/api/v4/comment_v5/...`).

use serde_json::Value;
use zhihu_types::{Author, Comment, CommentsPage};

fn string_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn number_at(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn author_of(value: Option<&Value>) -> Author {
    let Some(author) = value else {
        return Author {
            id: String::new(),
            name: "匿名用户".to_string(),
            headline: None,
            url_token: None,
            is_following: false,
            avatar_url: None,
            is_mcn: false,
        };
    };
    Author {
        id: author
            .get("id")
            .and_then(|id| {
                id.as_str()
                    .map(str::to_string)
                    .or_else(|| id.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default(),
        name: string_at(author, &["name"])
            .unwrap_or("匿名用户")
            .to_string(),
        headline: string_at(author, &["headline", "description"]).map(str::to_string),
        url_token: string_at(author, &["url_token"]).map(str::to_string),
        is_following: author
            .get("is_following")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        avatar_url: string_at(author, &["avatar_url", "avatarUrl"])
            .or_else(|| string_at(author, &["avatar_url_template", "avatarUrlTemplate"]))
            .map(|url| url.replace("{size}", "xl")),
        is_mcn: is_mcn_author(author),
    }
}

fn is_mcn_author(value: &Value) -> bool {
    crate::feed::is_mcn_author(value)
}

pub fn parse_comment(value: &Value) -> Option<Comment> {
    if value
        .get("is_delete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let id = value.get("id").and_then(|id| {
        id.as_str()
            .map(str::to_string)
            .or_else(|| id.as_i64().map(|n| n.to_string()))
    })?;
    let reply_to = value
        .get("reply_to_author")
        .and_then(|author| string_at(author, &["name"]))
        .map(str::to_string);

    Some(Comment {
        id,
        content: string_at(value, &["content"]).unwrap_or("").to_string(),
        author: author_of(value.get("author")),
        reply_to,
        like_count: number_at(value, &["like_count", "likeCount"]),
        child_count: number_at(value, &["child_comment_count", "childCommentCount"]),
        created_time: number_at(value, &["created_time", "createdTime"]) as i64,
        is_author: value
            .get("is_author")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_mine: value
            .get("is_mine")
            .or_else(|| {
                value
                    .get("relationship")
                    .and_then(|relationship| relationship.get("is_mine"))
            })
            .and_then(Value::as_bool)
            .unwrap_or(false),
        root_id: string_at(value, &["reply_root_comment_id", "root_comment_id"])
            .map(str::to_string),
        children: value
            .get("child_comments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_comment)
            .collect(),
        child_next_offset: value
            .get("child_comment_next_offset")
            .and_then(Value::as_u64),
    })
}

pub fn parse_comments(value: &Value) -> Vec<Comment> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_comment)
        .collect()
}

pub fn parse_comments_page(value: &Value) -> CommentsPage {
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    CommentsPage {
        items: parse_comments(value),
        is_end,
    }
}

/// Parse the legacy v4 comment list. Zhihu intermittently returns an empty
/// `data` array from the v5 `comment_v5` endpoint while v4 still returns the
/// full page, so the client falls back to this mapper for answers/articles.
pub fn parse_comments_v4_page(value: &Value) -> CommentsPage {
    let items: Vec<Comment> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let content = item.get("content").and_then(Value::as_str)?.to_string();
            let id = item
                .get("id")
                .and_then(|id| {
                    id.as_i64()
                        .map(|value| value.to_string())
                        .or_else(|| id.as_str().map(str::to_string))
                })
                .unwrap_or_default();
            let author = item.get("author").unwrap_or(&Value::Null);
            let member = author.get("member").unwrap_or(author);
            let name = member
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("匿名用户")
                .to_string();
            let reply_to = item
                .get("reply_to_author")
                .and_then(|reply| {
                    reply
                        .get("member")
                        .unwrap_or(reply)
                        .get("name")
                        .and_then(Value::as_str)
                })
                .map(str::to_string);
            Some(Comment {
                id,
                content,
                author: Author {
                    id: member
                        .get("id")
                        .and_then(|id| {
                            id.as_str()
                                .map(str::to_string)
                                .or_else(|| id.as_i64().map(|value| value.to_string()))
                        })
                        .unwrap_or_default(),
                    name,
                    headline: member
                        .get("headline")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    url_token: member
                        .get("url_token")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    is_following: false,
                    avatar_url: member
                        .get("avatar_url")
                        .and_then(Value::as_str)
                        .or_else(|| member.get("avatar_url_template").and_then(Value::as_str))
                        .map(|url| url.replace("{size}", "xl")),
                    is_mcn: is_mcn_author(member),
                },
                reply_to,
                like_count: item
                    .get("like_count")
                    .or_else(|| item.get("vote_count"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                child_count: item
                    .get("child_comment_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                created_time: item
                    .get("created_time")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                is_author: item
                    .get("is_author")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                is_mine: item
                    .get("is_mine")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                root_id: item
                    .get("reply_root_comment_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                children: Vec::new(),
                child_next_offset: None,
            })
        })
        .collect();
    let is_end = value
        .get("paging")
        .and_then(|paging| paging.get("is_end"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() < 20);
    CommentsPage { items, is_end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_children_and_paging() {
        let value = json!({
            "paging": { "is_end": false },
            "data": [{
                "id": "c1",
                "content": "root",
                "child_comment_count": 5,
                "child_comment_next_offset": 3,
                "child_comments": [{
                    "id": "c2",
                    "content": "child",
                    "author": { "name": "bob" }
                }]
            }]
        });
        let page = parse_comments_page(&value);
        assert!(!page.is_end);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].children.len(), 1);
        assert_eq!(page.items[0].children[0].author.name, "bob");
        assert_eq!(page.items[0].child_next_offset, Some(3));
    }

    #[test]
    fn parses_v4_comment_page() {
        let value = json!({
            "paging": { "is_end": true, "totals": 1 },
            "data": [{
                "id": 42,
                "content": "<p>hello</p>",
                "created_time": 1700000000,
                "like_count": 9,
                "author": { "member": { "name": "alice", "url_token": "alice" } },
                "reply_to_author": { "member": { "name": "bob" } }
            }]
        });
        let page = parse_comments_v4_page(&value);
        assert!(page.is_end);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "42");
        assert_eq!(page.items[0].author.name, "alice");
        assert_eq!(page.items[0].author.url_token.as_deref(), Some("alice"));
        assert_eq!(page.items[0].reply_to.as_deref(), Some("bob"));
        assert_eq!(page.items[0].like_count, 9);
    }

    #[test]
    fn parses_comment_list() {
        let value = json!({
            "data": [{
                "id": "c1",
                "content": "<p>hello</p>",
                "created_time": 1700000000,
                "like_count": 3,
                "child_comment_count": 2,
                "is_author": true,
                "author": { "id": "u1", "name": "alice" },
                "reply_to_author": { "name": "bob" }
            }, {
                "id": "c2",
                "is_delete": true,
                "content": "gone"
            }]
        });
        let comments = parse_comments(&value);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, "c1");
        assert_eq!(comments[0].author.name, "alice");
        assert_eq!(comments[0].reply_to.as_deref(), Some("bob"));
        assert_eq!(comments[0].child_count, 2);
    }
}
