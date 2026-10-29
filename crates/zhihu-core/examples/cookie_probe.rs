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
