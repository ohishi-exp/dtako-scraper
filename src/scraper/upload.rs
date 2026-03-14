use std::path::PathBuf;

use tracing::{error, info, warn};

use crate::error::ScraperError;

/// daiun-salary の /internal/upload に ZIP ファイルを送信
/// 失敗時は /internal/store で R2 に退避
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

    // 1. まず /internal/upload を試行
    let url = format!("{}/internal/upload", daiun_salary_url);
    info!("Uploading {:?} to {} (tenant={})", zip_path, url, tenant_id);

    match send_multipart(&url, tenant_id, &filename, &file_bytes).await {
        Ok(body) => {
            info!("Upload successful: {}", body);
            return Ok(body);
        }
        Err(e) => {
            warn!("Upload failed: {e}, attempting fallback store to R2...");
        }
    }

    // 2. フォールバック: /internal/store で R2 に退避
    let store_url = format!("{}/internal/store", daiun_salary_url);
    info!("Storing ZIP to R2 via {} (tenant={})", store_url, tenant_id);

    match send_multipart(&store_url, tenant_id, &filename, &file_bytes).await {
        Ok(body) => {
            warn!("ZIP stored for later rerun: {}", body);
            Ok(format!("STORED_FOR_RETRY: {}", body))
        }
        Err(e) => {
            error!("Both upload and store failed: {e}");
            Err(ScraperError::Upload(format!(
                "Upload and fallback store both failed: {e}"
            )))
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
