//! Live acceptance against a logged-in cookie profile. Manual only.
//!
//! ```bash
//! cargo run -p zhihu-core --example cookie_probe
//! cargo run -p zhihu-core --example cookie_probe -- C:\path\to\zhihu-rust
//! ```

use zhihu_api::ZhihuClient;
use zhihu_store::{Store, DB_FILE_NAME};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("APPDATA")
                .ok()
                .map(|base| std::path::PathBuf::from(base).join("zhihu-rust"))
        })
        .ok_or("pass the data directory as the first argument")?;
    println!("data dir: {}", dir.display());

    let store = Store::open(dir.join(DB_FILE_NAME))?;
    let cookies = store.load_cookies(0)?;
    println!(
        "cookies: {} ({:?})",
        cookies.len(),
        cookies
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    let client = ZhihuClient::new()?;
    client.set_cookies(cookies);

    if std::env::var("ZHIHU_MCN_PROBE").is_ok() {
        fn scan(value: &serde_json::Value, path: &str, hits: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, item) in map {
                        let next = format!("{path}.{key}");
                        if key.to_lowercase().contains("mcn") {
                            hits.push(format!("{next} = {item}"));
                        }
                        scan(item, &next, hits);
                    }
                }
                serde_json::Value::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        scan(item, &format!("{path}[{index}]"), hits);
                    }
                }
                serde_json::Value::String(text) if text.to_lowercase().contains("mcn") => {
                    hits.push(format!("{path} = {text:?}"));
                }
                _ => {}
            }
        }
        let mut targets = vec![
            "https://www.zhihu.com/api/v3/feed/topstory/recommend?desktop=true&limit=20"
                .to_string(),
            "https://www.zhihu.com/api/v3/moments?limit=20&desktop=true".to_string(),
        ];
        for (index, query) in ["MCN", "MCN机构", "知乎 MCN"].iter().enumerate() {
            targets.push(format!(
                "https://www.zhihu.com/api/v4/search_v3?t=general&q={}&correction=1&offset=0&limit=20&search_source=Normal",
                query.replace(' ', "%20")
            ));
            let _ = index;
        }
        for url in targets {
            match client.get_json(&url, true).await {
                Ok(value) => {
                    let mut hits = Vec::new();
                    scan(&value, "root", &mut hits);
                    println!("MCN probe {url}: {} hits", hits.len());
                    for hit in hits.into_iter().take(30) {
                        println!("  {hit}");
                    }
                }
                Err(error) => println!("MCN probe {url} failed: {error}"),
            }
        }
        return Ok(());
    }

    if let Ok(url) = std::env::var("ZHIHU_PROBE_TEXT_URL") {
        match client.get_text(&url).await {
            Ok(text) => println!(
                "text probe {url} OK: {} bytes, head={:?}",
                text.len(),
                text.chars().take(500).collect::<String>()
            ),
            Err(error) => println!("text probe {url} failed: {error}"),
        }
        return Ok(());
    }

    if let Ok(url) = std::env::var("ZHIHU_PROBE_URL") {
        match client.get_json(&url, true).await {
            Ok(value) => println!(
                "probe {url} OK: {}",
                serde_json::to_string(&value)?
                    .chars()
                    .take(2000)
                    .collect::<String>()
            ),
            Err(error) => println!("probe {url} failed: {error}"),
        }
        return Ok(());
    }

    match client.me_profile().await {
        Ok(profile) => println!("me OK: {} ({:?})", profile.name, profile.url_token),
        Err(error) => println!("me failed: {error}"),
    }

    let feed = match client.feed_recommend_items(1).await {
        Ok((items, is_end)) => {
            println!("feed OK: {} items, is_end={is_end}", items.len());
            items
        }
        Err(error) => {
            println!("feed failed: {error}");
            Vec::new()
        }
    };
    match client.feed_recommend_items(2).await {
        Ok((items, is_end)) => println!("feed page 2: {} items, is_end={is_end}", items.len()),
        Err(error) => println!("feed page 2 failed: {error}"),
    }

    for kind in [
        zhihu_types::FollowFeedKind::Recommend,
        zhihu_types::FollowFeedKind::Activity,
    ] {
        match client.follow_feed(kind, None).await {
            Ok(page) => {
                println!(
                    "follow {} OK: {} items, is_end={}, next={:?}, first={:?}",
                    kind.label(),
                    page.items.len(),
                    page.is_end,
                    page.next,
                    page.items.first().map(|item| item.title.as_str())
                );
                if let Some(next) = page.next {
                    match client.follow_feed(kind, Some(&next)).await {
                        Ok(page) => println!(
                            "follow {} page 2: {} items, is_end={}",
                            kind.label(),
                            page.items.len(),
                            page.is_end
                        ),
                        Err(error) => println!("follow {} page 2 failed: {error}", kind.label()),
                    }
                }
            }
            Err(error) => println!("follow {} failed: {error}", kind.label()),
        }
    }

    let topic_id = feed
        .iter()
        .flat_map(|item| item.topic_refs.iter())
        .find(|topic| !topic.id.is_empty())
        .map(|topic| topic.id.clone())
        .unwrap_or_else(|| "19776749".to_string());
    match client.topic_detail(&topic_id).await {
        Ok(topic) => println!(
            "topic detail OK: {} / {} followers / following={}",
            topic.name, topic.followers_count, topic.is_following
        ),
        Err(error) => println!("topic detail failed: {error}"),
    }
    for (tab, sort) in [
        (
            zhihu_types::TopicFeedTab::Discussion,
            zhihu_types::TopicSort::Essence,
        ),
        (
            zhihu_types::TopicFeedTab::Ideas,
            zhihu_types::TopicSort::Latest,
        ),
        (
            zhihu_types::TopicFeedTab::Unanswered,
            zhihu_types::TopicSort::Hot,
        ),
    ] {
        match client.topic_feed(&topic_id, tab, sort, None).await {
            Ok(page) => println!(
                "topic {} OK: {} items, is_end={}, first={:?}",
                tab.label(),
                page.items.len(),
                page.is_end,
                page.items.first().map(|item| item.title.as_str())
            ),
            Err(error) => println!("topic {} failed: {error}", tab.label()),
        }
    }

    let detail = feed
        .iter()
        .find(|item| item.kind == zhihu_types::ContentKind::Answer)
        .cloned();
    if let Some(item) = detail {
        println!("first answer: {} / {}", item.id, item.title);
        match client.answer_detail(&item.id).await {
            Ok(detail) => println!(
                "detail OK: {} chars of HTML, {} votes, {} comments",
                detail.content_html.len(),
                detail.vote_up_count,
                detail.comment_count
            ),
            Err(error) => println!("detail failed: {error}"),
        }
        match client
            .comments(zhihu_types::ContentKind::Answer, &item.id, 0)
            .await
        {
            Ok(page) => {
                println!(
                    "comments OK: {} root comments, is_end={}",
                    page.items.len(),
                    page.is_end
                );
                if let Some(first) = page.items.first() {
                    println!(
                        "comment first: id={} child_count={} children={} content={:?}",
                        first.id,
                        first.child_count,
                        first.children.len(),
                        first.content.chars().take(60).collect::<String>()
                    );
                    match client.child_comments(&first.id, 0).await {
                        Ok(children) => println!(
                            "child comments OK: {} items, is_end={}",
                            children.items.len(),
                            children.is_end
                        ),
                        Err(error) => println!("child comments failed: {error}"),
                    }
                }
            }
            Err(error) => println!("comments failed: {error}"),
        }
    }

    match client.notifications(0).await {
        Ok(page) => println!(
            "notifications OK: {} items, is_end={}",
            page.items.len(),
            page.is_end
        ),
        Err(error) => println!("notifications failed: {error}"),
    }

    if std::env::var("ZHIHU_MARK_READ").is_ok() {
        match client.mark_all_notifications_read().await {
            Ok(()) => println!("mark all read OK"),
            Err(error) => println!("mark all read failed: {error}"),
        }
    }

    for (label, options) in [
        ("all", zhihu_api::SearchOptions::default()),
        (
            "answers",
            zhihu_api::SearchOptions {
                vertical: zhihu_api::SearchVertical::Answer,
                sort: zhihu_api::SearchSort::Default,
            },
        ),
        (
            "articles",
            zhihu_api::SearchOptions {
                vertical: zhihu_api::SearchVertical::Article,
                sort: zhihu_api::SearchSort::Default,
            },
        ),
        (
            "latest",
            zhihu_api::SearchOptions {
                vertical: zhihu_api::SearchVertical::All,
                sort: zhihu_api::SearchSort::Latest,
            },
        ),
    ] {
        match client.search("rust", 0, options).await {
            Ok(page) => println!(
                "search {label} OK: {} results, is_end={}",
                page.items.len(),
                page.is_end
            ),
            Err(error) => println!("search {label} failed: {error}"),
        }
    }

    Ok(())
}
