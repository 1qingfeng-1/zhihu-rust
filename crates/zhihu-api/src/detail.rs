//! Answer / article detail mapping.

use serde_json::Value;
use zhihu_types::{Author, ContentDetail, ContentKind, MemberProfile, QuestionDetail};

fn string_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn number_at(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn id_of(value: &Value) -> Option<String> {
    value.get("id").and_then(|id| {
        id.as_i64()
            .map(|n| n.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    })
}

fn author_of(value: &Value) -> Author {
    let author = value.get("author");
    Author {
        id: author
            .and_then(|a| a.get("id"))
            .and_then(|id| {
                id.as_str()
                    .map(str::to_string)
                    .or_else(|| id.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default(),
        name: author
            .and_then(|a| string_at(a, &["name"]))
            .unwrap_or("匿名用户")
            .to_string(),
        headline: author
            .and_then(|a| string_at(a, &["headline", "description"]))
            .map(str::to_string),
        url_token: author
            .and_then(|a| string_at(a, &["url_token"]))
            .map(str::to_string),
        is_following: author
            .and_then(|a| a.get("is_following"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        avatar_url: author
            .and_then(|a| string_at(a, &["avatar_url", "avatarUrl"]))
            .or_else(|| {
                author.and_then(|a| string_at(a, &["avatar_url_template", "avatarUrlTemplate"]))
            })
            .map(|url| url.replace("{size}", "xl")),
        is_mcn: value.get("author").is_some_and(crate::feed::is_mcn_author),
    }
}

fn vote_state_of(value: &Value) -> String {
    if value
        .get("virtuals")
        .and_then(|virtuals| virtuals.get("is_liked").or_else(|| virtuals.get("isLiked")))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "up".to_string();
    }
    value
        .get("reaction")
        .and_then(|reaction| reaction.get("relation"))
        .and_then(|relation| relation.get("vote"))
        .and_then(Value::as_str)
        .unwrap_or("neutral")
        .to_string()
}

fn question_id_of(value: &Value) -> Option<String> {
    let id = value
        .get("question")
        .and_then(|question| question.get("id"))?;
    id.as_i64()
        .map(|id| id.to_string())
        .or_else(|| id.as_str().map(str::to_string))
}

pub fn parse_answer(value: &Value) -> Option<ContentDetail> {
    Some(ContentDetail {
        id: id_of(value)?,
        title: value
            .get("question")
            .and_then(|q| string_at(q, &["title"]))
            .unwrap_or("回答")
            .to_string(),
        author: author_of(value),
        kind: ContentKind::Answer,
        content_html: string_at(value, &["content"])
            .unwrap_or_default()
            .to_string(),
        excerpt: string_at(value, &["excerpt"])
            .unwrap_or_default()
            .to_string(),
        vote_up_count: number_at(value, &["voteup_count", "voteupCount"]),
        comment_count: number_at(value, &["comment_count", "commentCount"]),
        vote_state: vote_state_of(value),
        created_time: number_at(value, &["created_time", "created"]) as i64,
        updated_time: number_at(value, &["updated_time", "updated"]) as i64,
        question_id: question_id_of(value),
    })
}

pub fn parse_article(value: &Value) -> Option<ContentDetail> {
    Some(ContentDetail {
        id: id_of(value)?,
        title: string_at(value, &["title"]).unwrap_or("文章").to_string(),
        author: author_of(value),
        kind: ContentKind::Article,
        content_html: string_at(value, &["content"])
            .unwrap_or_default()
            .to_string(),
        excerpt: string_at(value, &["excerpt"])
            .unwrap_or_default()
            .to_string(),
        vote_up_count: number_at(value, &["voteup_count", "voteupCount"]),
        comment_count: number_at(value, &["comment_count", "commentCount"]),
        vote_state: vote_state_of(value),
        created_time: number_at(value, &["created_time", "created"]) as i64,
        updated_time: number_at(value, &["updated_time", "updated"]) as i64,
        question_id: question_id_of(value),
    })
}

/// Convert the rich-text array returned by `/api/v4/pins/{id}` into the HTML
/// subset consumed by the shared content pipeline. Text-only pins are escaped
/// as well: their content is data, never trusted markup.
pub fn pin_content_html(value: &Value) -> String {
    fn escape_html(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    if let Some(html) = string_at(value, &["content_html", "contentHtml"]) {
        return html.to_string();
    }
    if let Some(text) = string_at(value, &["content"]) {
        return format!("<p>{}</p>", escape_html(text));
    }
    value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| {
            Some(match string_at(block, &["type"]) {
                Some("image") => {
                    let url = string_at(block, &["original_url", "url"])?;
                    format!("<figure><img src=\"{}\" /></figure>", escape_html(url))
                }
                Some("link" | "link_card") => {
                    let url = string_at(block, &["url"])?;
                    let label = string_at(block, &["content", "text", "title"]).unwrap_or(url);
                    format!("<p><a href=\"{}\">{}</a></p>", escape_html(url), label)
                }
                _ => {
                    let html = string_at(block, &["content", "text"])?;
                    format!("<p>{html}</p>")
                }
            })
        })
        .collect()
}

pub fn parse_pin(value: &Value) -> Option<ContentDetail> {
    let content_html = pin_content_html(value);
    Some(ContentDetail {
        id: id_of(value)?,
        title: string_at(value, &["title", "excerpt_title"])
            .filter(|title| !title.is_empty())
            .unwrap_or("想法")
            .to_string(),
        author: author_of(value),
        kind: ContentKind::Pin,
        content_html,
        excerpt: string_at(value, &["excerpt", "excerpt_title"])
            .unwrap_or_default()
            .to_string(),
        vote_up_count: number_at(value, &["voteup_count", "like_count", "voteupCount"]),
        comment_count: number_at(value, &["comment_count", "commentCount"]),
        vote_state: vote_state_of(value),
        created_time: number_at(value, &["created_time", "created"]) as i64,
        updated_time: number_at(value, &["updated_time", "updated"]) as i64,
        question_id: None,
    })
}

pub fn parse_question(value: &Value) -> Option<QuestionDetail> {
    Some(QuestionDetail {
        id: id_of(value)?,
        title: string_at(value, &["title"]).unwrap_or("问题").to_string(),
        detail_html: string_at(value, &["detail", "content"])
            .unwrap_or_default()
            .to_string(),
        answer_count: number_at(value, &["answer_count", "answerCount"]),
        follower_count: number_at(value, &["follower_count", "followerCount"]),
    })
}

pub fn parse_member(value: &Value) -> Option<MemberProfile> {
    let id = id_of(value)?;
    let url_token = string_at(value, &["url_token", "urlToken"])
        .unwrap_or(&id)
        .to_string();
    Some(MemberProfile {
        id,
        name: string_at(value, &["name"])
            .unwrap_or("知乎用户")
            .to_string(),
        url_token,
        avatar_url: string_at(value, &["avatar_url", "avatarUrl"]).map(str::to_string),
        headline: string_at(value, &["headline"]).map(str::to_string),
        description: string_at(value, &["description"])
            .unwrap_or_default()
            .to_string(),
        follower_count: number_at(value, &["follower_count", "followerCount"]),
        following_count: number_at(value, &["following_count", "followingCount"]),
        answer_count: number_at(value, &["answer_count", "answerCount"]),
        articles_count: number_at(value, &["articles_count", "articlesCount"]),
        is_following: value
            .get("is_following")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_blocking: value
            .get("is_blocking")
            .or_else(|| value.get("is_blocked"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_mcn: crate::feed::is_mcn_author(value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_pin_content_and_like_state() {
        let value = json!({
            "id": 101,
            "excerpt_title": "想法标题",
            "content": [
                { "type": "text", "content": "这是 <b>想法</b>。", "title": "想法标题" },
                { "type": "image", "url": "https://picx.zhimg.com/pin.jpg", "original_url": "https://picx.zhimg.com/original.jpg" },
                { "type": "link_card", "url": "https://www.zhihu.com/question/42" }
            ],
            "like_count": 9,
            "comment_count": 3,
            "created": 1713456789,
            "updated": 1713456790,
            "virtuals": { "is_liked": true },
            "author": { "id": "u1", "name": "alice", "url_token": "alice" }
        });
        let detail = parse_pin(&value).unwrap();
        assert_eq!(detail.kind, ContentKind::Pin);
        assert_eq!(detail.title, "想法标题");
        assert_eq!(detail.vote_up_count, 9);
        assert_eq!(detail.vote_state, "up");
        assert!(detail.content_html.contains("<b>想法</b>"));
        assert!(detail.content_html.contains("original.jpg"));
        assert!(detail.content_html.contains("question/42"));
    }

    #[test]
    fn pin_content_html_escapes_text_and_supports_legacy_html() {
        let html = pin_content_html(&json!({ "content": "<script>alert(1)</script>" }));
        assert_eq!(html, "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>");
        assert_eq!(
            pin_content_html(&json!({ "content_html": "<p>safe</p>" })),
            "<p>safe</p>"
        );
    }

    #[test]
    fn parses_answer() {
        let value = json!({
            "id": 7,
            "content": "<p>hello</p>",
            "excerpt": "hello",
            "voteup_count": 12,
            "comment_count": 3,
            "author": { "name": "alice" },
            "question": { "id": 42, "title": "why?" },
            "reaction": { "relation": { "vote": "up" } }
        });
        let detail = parse_answer(&value).unwrap();
        assert_eq!(detail.vote_state, "up");
        assert_eq!(detail.id, "7");
        assert_eq!(detail.title, "why?");
        assert_eq!(detail.kind, ContentKind::Answer);
        assert_eq!(detail.vote_up_count, 12);
        assert_eq!(detail.content_html, "<p>hello</p>");
        assert_eq!(detail.question_id.as_deref(), Some("42"));
    }

    #[test]
    fn parses_article() {
        let value = json!({
            "id": 9,
            "title": "article",
            "content": "<p>body</p>",
            "author": {
                "id": 11,
                "name": "bob",
                "headline": "writer",
                "url_token": "bob-token",
                "is_following": true
            }
        });
        let detail = parse_article(&value).unwrap();
        assert_eq!(detail.kind, ContentKind::Article);
        assert_eq!(detail.author.name, "bob");
        assert_eq!(detail.author.id, "11");
        assert_eq!(detail.author.url_token.as_deref(), Some("bob-token"));
        assert!(detail.author.is_following);
    }
}
