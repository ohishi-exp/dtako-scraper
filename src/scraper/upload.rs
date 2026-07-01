use std::path::PathBuf;

use tracing::{error, info};

use crate::config::DeviceCredential;
use crate::device_auth::mint_device_token_for_tenant;
use crate::error::ScraperError;

/// rust-alc-api の `POST /api/upload` に、auth-worker の `/device-data-proxy` 経由で
/// ZIP ファイルを送信する。
///
/// rust-alc-api 本番 Cloud Run は #434 lockdown で `allUsers` invoker 権限が撤去済みのため、
/// 直接 HTTP POST は Google Front End レベルで 403 Forbidden になる (2026-07-01 に判明)。
/// device credential (`DTAKO_DEVICE_CREDENTIALS`、tenant_id ごとに 1 組) で device JWT を
/// mint し、`{AUTH_WORKER_URL}/device-data-proxy/api/upload` を `Authorization: Bearer <jwt>`
/// で叩く (browser-render-rust の dtakolog 送信と同じ device-dtako-ingest role を共用、
/// Refs ippoan/auth-worker#341, rust-alc-api#434)。
///
/// device-data-proxy は JWT に焼き込まれた `tenant_id` claim を信頼して `X-Tenant-ID` を
/// 注入するため、client 側の `X-Tenant-ID` ヘッダーや multipart `tenant_id` フィールドは
/// 送らない (送っても proxy 側で無視される)。
pub async fn upload_zip(
    auth_worker_url: &str,
    credential: &DeviceCredential,
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

    let device_jwt = mint_device_token_for_tenant(
        auth_worker_url,
        credential,
        tenant_id,
        std::time::Duration::from_secs(30),
    )
    .await
    .map_err(|e| ScraperError::Upload(format!("device token mint failed: {e}")))?;

    let url = format!(
        "{}/device-data-proxy/api/upload",
        auth_worker_url.trim_end_matches('/')
    );
    info!("Uploading {:?} to {} (tenant={})", zip_path, url, tenant_id);

    match send_multipart(&url, &device_jwt, &filename, &file_bytes).await {
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
    device_jwt: &str,
    filename: &str,
    file_bytes: &[u8],
) -> Result<String, String> {
    let file_part = reqwest::multipart::Part::bytes(file_bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("application/zip")
        .map_err(|e| format!("MIME: {e}"))?;

    let form = reqwest::multipart::Form::new().part("file", file_part);

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {device_jwt}"))
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
