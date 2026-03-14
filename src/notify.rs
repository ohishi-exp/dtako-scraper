use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use tracing::{error, info};

use crate::config::MailConfig;

pub async fn send_result_mail(config: &MailConfig, subject: &str, body: &str) {
    let email = match Message::builder()
        .from(config.smtp_user.parse().unwrap())
        .to(config.to.parse().unwrap())
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
    {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to build email: {e}");
            return;
        }
    };

    let creds = Credentials::new(config.smtp_user.clone(), config.smtp_pass.clone());

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay("smtp.gmail.com")
        .unwrap()
        .credentials(creds)
        .build();

    match mailer.send(email).await {
        Ok(_) => info!("Result email sent to {}", config.to),
        Err(e) => error!("Failed to send email: {e}"),
    }
}
