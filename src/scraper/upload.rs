use std::path::PathBuf;

use tracing::info;

use crate::error::ScraperError;

/// daiun-salary の /internal/upload に ZIP ファイルを送信
pub async fn upload_zip(
    daiun_salary_url: &str,
    tenant_id: &str,
    zip_path: &PathBuf,
) -> Result<String, ScraperError> {
    let url = format!("{}/internal/upload", daiun_salary_url);
    info!("Uploading {:?} to {} (tenant={})", zip_path, url, tenant_id);

    let file_bytes =
        std::fs::read(zip_path).map_err(|e| ScraperError::Upload(format!("Read file: {e}")))?;

    let filename = zip_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(filename)
        .mime_str("application/zip")
        .map_err(|e| ScraperError::Upload(format!("MIME: {e}")))?;

    let form = reqwest::multipart::Form::new()
        .text("tenant_id", tenant_id.to_string())
        .part("file", file_part);

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| ScraperError::Upload(format!("Request: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ScraperError::Upload(format!("Response body: {e}")))?;

    if !status.is_success() {
        return Err(ScraperError::Upload(format!(
            "Status {}: {}",
            status, body
        )));
    }

    info!("Upload successful: {}", body);
    Ok(body)
}
