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
    }
}

fn vote_state_of(value: &Value) -> String {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
