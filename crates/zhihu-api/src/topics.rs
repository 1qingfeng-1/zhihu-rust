//! Topic detail and feed mapping.

use serde_json::Value;
use zhihu_types::TopicDetail;

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
    value
        .get("id")
        .or_else(|| value.get("topic_id"))
        .and_then(|id| {
            id.as_i64()
                .map(|n| n.to_string())
                .or_else(|| id.as_str().map(str::to_string))
        })
}

pub fn parse_topic_detail(value: &Value, requested_id: &str) -> TopicDetail {
    TopicDetail {
        id: id_of(value).unwrap_or_else(|| requested_id.to_string()),
        name: string_at(value, &["name", "title"])
            .unwrap_or("话题")
            .to_string(),
        excerpt: string_at(value, &["excerpt", "description"])
            .unwrap_or_default()
            .to_string(),
        avatar_url: string_at(value, &["avatar_url", "avatarUrl"]).map(str::to_string),
        followers_count: number_at(value, &["followers_count", "followersCount"]),
        questions_count: number_at(value, &["questions_count", "questionsCount"]),
        discuss_count: number_at(value, &["discuss_count", "discussCount"]),
        is_following: value
            .get("is_following")
            .or_else(|| value.get("isFollowing"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_topic_detail_with_numeric_topic_id() {
        let value = json!({
            "topic_id": 19776749,
            "name": "Rust",
            "excerpt": "系统编程语言",
            "avatar_url": "https://picx.zhimg.com/rust.jpg",
            "followers_count": 1200,
            "questions_count": 34,
            "discuss_count": 8800,
            "is_following": true
        });
        let topic = parse_topic_detail(&value, "19776749");
        assert_eq!(topic.id, "19776749");
        assert_eq!(topic.name, "Rust");
        assert_eq!(topic.followers_count, 1200);
        assert!(topic.is_following);
    }

    #[test]
    fn falls_back_to_requested_topic_id() {
        let topic = parse_topic_detail(&json!({ "name": "GPUI" }), "abc");
        assert_eq!(topic.id, "abc");
        assert_eq!(topic.excerpt, "");
    }
}
