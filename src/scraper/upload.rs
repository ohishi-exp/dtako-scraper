use std::path::PathBuf;

use tracing::{error, info};

use crate::error::ScraperError;

/// rust-alc-api (env var 名は `DAIUN_SALARY_URL` だが実体は rust-alc-api の Cloud Run URL) の
/// `POST /api/upload` (crates/alc-dtako/src/dtako_upload.rs::upload_zip、require_tenant_header
/// 配下) に ZIP ファイルを送信する。`/internal/upload` は daiun-salary (別リポジトリ) のパスで
/// rust-alc-api には存在しないため誤り (2026-07-01 の誤修正、以降訂正)。
pub async fn upload_zip(
    daiun_salary_url: &str,
    tenant_id: &str,
    zip_path: &PathBuf,
) -> Result<String, ScraperError> {
    let file_bytes =
        std::fs::read(zip_path).map_err(|e| ScraperError::Upload(format!("Read file: {e}")))?;

    let filename = zip_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let url = format!("{}/api/upload", daiun_salary_url);
    info!("Uploading {:?} to {} (tenant={})", zip_path, url, tenant_id);

    match send_multipart(&url, tenant_id, &filename, &file_bytes).await {
        Ok(body) => {
            info!("Upload successful: {}", body);
            Ok(body)
        }
        Err(e) => {
            error!("Upload failed: {e}");
            Err(ScraperError::Upload(format!("Upload failed: {e}")))
        }
    }
}

async fn send_multipart(
    url: &str,
    tenant_id: &str,
    filename: &str,
    file_bytes: &[u8],
) -> Result<String, String> {
    let file_part = reqwest::multipart::Part::bytes(file_bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("application/zip")
        .map_err(|e| format!("MIME: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text("tenant_id", tenant_id.to_string())
        .part("file", file_part);

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("X-Tenant-ID", tenant_id)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("Request: {e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Response body: {e}"))?;

    if !status.is_success() {
        return Err(format!("Status {}: {}", status, body));
    }

    Ok(body)
}
