use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use tracing::{error, info, warn};

use crate::config::MailConfig;

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_SECS: u64 = 5;

fn build_email(config: &MailConfig, subject: &str, body: &str) -> Result<Message, String> {
    Message::builder()
        .from(config.smtp_user.parse().unwrap())
        .to(config.to.parse().unwrap())
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| e.to_string())
}

pub async fn send_result_mail(config: &MailConfig, subject: &str, body: &str) {
    let creds = Credentials::new(config.smtp_user.clone(), config.smtp_pass.clone());
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay("smtp.gmail.com")
        .unwrap()
        .credentials(creds)
        .build();

    for attempt in 0..=MAX_RETRIES {
        let email = match build_email(config, subject, body) {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to build email: {e}");
                return;
            }
        };

        match mailer.send(email).await {
            Ok(_) => {
                info!("Result email sent to {}", config.to);
                return;
            }
            Err(e) => {
                if attempt < MAX_RETRIES {
                    let delay = INITIAL_BACKOFF_SECS * 2u64.pow(attempt);
                    warn!("Failed to send email (attempt {}/{}): {e}. Retrying in {delay}s...", attempt + 1, MAX_RETRIES + 1);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                } else {
                    error!("Failed to send email after {} attempts: {e}", MAX_RETRIES + 1);
                }
            }
        }
    }
}
