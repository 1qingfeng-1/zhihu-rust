//! Manual SMS-login probe. Not run in CI.
//!
//! Pass a phone number only if you expect to receive an SMS:
//!
//! ```bash
//! cargo run -p zhihu-api --example phone_probe            # fake number, no SMS
//! cargo run -p zhihu-api --example phone_probe -- 138...  # sends a real code
//! ```

use zhihu_api::PhoneLoginSession;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let phone = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000000000".to_string());
    let mut session = PhoneLoginSession::new()?;

    session.initialize().await?;
    println!("guest init OK");

    match session.request_digits(&phone).await {
        Ok(zhihu_api::DigitsOutcome::Sent) => println!("digits sent to {phone}"),
        Ok(zhihu_api::DigitsOutcome::CaptchaRequired { image_base64 }) => println!(
            "captcha required (image bytes: {:?})",
            image_base64.map(|image| image.len())
        ),
        Err(error) => println!("request_digits rejected: {error}"),
    }

    Ok(())
}
