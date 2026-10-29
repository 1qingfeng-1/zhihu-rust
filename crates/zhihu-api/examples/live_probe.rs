//! Manual live probe. Not run in CI.
//!
//! ```bash
//! cargo run -p zhihu-api --example live_probe
//! ```

use zhihu_api::ZhihuClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ZhihuClient::new()?;
    println!("UA: {}", client.user_agent());

    client.bootstrap().await?;
    println!("cookie header: {:?}", client.cookie_header());
    println!("d_c0: {:?}", client.dc0());

    match client.hot_search().await {
        Ok(items) => println!(
            "hot_search OK: {} items, first={:?}",
            items.len(),
            items.first().map(|item| item.query.as_str())
        ),
        Err(error) => println!("hot_search failed: {error}"),
    }

    let qr_client = ZhihuClient::new()?;
    match qr_client.qr_login_start().await {
        Ok(session) => println!(
            "qr start OK: token={} link={} expires={}",
            session.token, session.link, session.expires_at
        ),
        Err(error) => println!("qr start failed: {error}"),
    }

    match client
        .search_json("rust", 0, zhihu_api::SearchOptions::default())
        .await
    {
        Ok(value) => println!(
            "search OK: {}",
            serde_json::to_string(&value)?
                .chars()
                .take(900)
                .collect::<String>()
        ),
        Err(error) => println!("search failed: {error}"),
    }

    match client.daily_latest().await {
        Ok(items) => println!(
            "daily OK: {} items, first={:?}",
            items.len(),
            items.first().map(|item| item.title.as_str())
        ),
        Err(error) => println!("daily failed: {error}"),
    }

    // Signed request against an endpoint that works anonymously: if the
    // signature were rejected we would see a 403/100 instead of 200.
    match client
        .get_json(
            "https://www.zhihu.com/api/v4/search/hot_search?limit=5",
            true,
        )
        .await
    {
        Ok(_) => println!("signed hot_search OK"),
        Err(error) => println!("signed hot_search failed: {error}"),
    }

    for url in [
        "https://www.zhihu.com/api/v4/questions/19550517",
        "https://www.zhihu.com/api/v4/questions/19550517/answers?limit=5&offset=0",
        "https://www.zhihu.com/api/v4/answers/2035661632110585441",
    ] {
        match client.get_json(url, true).await {
            Ok(value) => println!(
                "signed {url} OK: {}",
                serde_json::to_string(&value)?
                    .chars()
                    .take(200)
                    .collect::<String>()
            ),
            Err(error) => println!("signed {url} failed: {error}"),
        }
    }

    let mut phone = zhihu_api::PhoneLoginSession::new()?;
    match phone.initialize().await {
        Ok(()) => println!("phone guest init OK"),
        Err(error) => println!("phone guest init failed: {error}"),
    }
    match phone.captcha_available().await {
        Ok(show) => println!("phone captcha state OK: show_captcha={show}"),
        Err(error) => println!("phone captcha state failed: {error}"),
    }

    match client.feed_recommend(1).await {
        Ok(value) => println!(
            "recommend OK: {}",
            serde_json::to_string(&value)?
                .chars()
                .take(500)
                .collect::<String>()
        ),
        Err(error) => println!("recommend failed: {error}"),
    }

    Ok(())
}
