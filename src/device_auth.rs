//! device JWT を発行し、auth-worker `/device-data-proxy` 経由で rust-alc-api
//! (Cloud Run IAM lockdown 後) にアクセスするための helper。
//!
//! dtako-scraper はブラウザセッションを持たない無人サービスのため、
//! auth-worker の device-token 基盤 (ohishi-exp/smb-watch#1 Phase 2、
//! ippoan/auth-worker#333、ippoan/auth-worker#341 で role を dtako 系で共用化) を使う:
//! `device_id`/`device_secret` (pairing 時に 1 回発行、tenant に紐付け済み) を
//! `POST /device/token` に渡し、短命 device JWT を得る。tenant は device record 由来で
//! client からは指定できない (`X-Tenant-ID` の詐称防止、rust-alc-api#434 followup)。
//!
//! dtako-scraper は `DTAKO_ACCOUNTS` の各企業が別 tenant_id を持つため、
//! 1 device credential = 1 tenant_id の前提が崩れないよう、mint 結果の
//! `tenant_id` を呼び出し側が期待する tenant_id と assert する
//! (`mint_device_token_for_tenant`)。これが VPS `.env` の設定ミスで
//! 別テナントにアップロードしてしまう事故に対する唯一のクライアント側防御。

use serde::Deserialize;

use crate::config::DeviceCredential;

#[derive(Deserialize)]
struct DeviceTokenResponse {
    access_token: String,
    tenant_id: String,
}

/// `device_id` + `device_secret` を device JWT に交換し、`expected_tenant_id` と
/// 応答の `tenant_id` が一致することを確認した上で JWT を返す。
/// 呼び出し側 (upload_zip) が毎回 fresh に mint する想定 (TTL 1h、送信頻度は低いので
/// キャッシュしない)。
pub async fn mint_device_token_for_tenant(
    auth_worker_url: &str,
    credential: &DeviceCredential,
    expected_tenant_id: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    if auth_worker_url.is_empty() {
        return Err("AUTH_WORKER_URL not configured".to_string());
    }
    if credential.device_id.is_empty() || credential.device_secret.is_empty() {
        return Err("device_id / device_secret not configured".to_string());
    }

    let url = format!("{}/device/token", auth_worker_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "device_id": credential.device_id,
            "device_secret": credential.device_secret,
        }))
        .send()
        .await
        .map_err(|e| format!("device/token request failed: {}", e))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read device/token response: {}", e))?;

    if !status.is_success() {
        return Err(format!("device/token returned {}: {}", status, text));
    }

    let body: DeviceTokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse device/token response: {} body={}", e, text))?;

    if body.tenant_id != expected_tenant_id {
        return Err(format!(
            "device credential tenant mismatch: expected={} got={} (VPS の DTAKO_DEVICE_CREDENTIALS \
             設定ミスの可能性。要確認)",
            expected_tenant_id, body.tenant_id
        ));
    }

    Ok(body.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn credential() -> DeviceCredential {
        DeviceCredential {
            device_id: "device-1".to_string(),
            device_secret: "secret-1".to_string(),
        }
    }

    #[tokio::test]
    async fn errors_when_auth_worker_url_unset() {
        let err =
            mint_device_token_for_tenant("", &credential(), "tenant-1", Duration::from_secs(5))
                .await
                .unwrap_err();
        assert!(err.contains("AUTH_WORKER_URL"));
    }

    #[tokio::test]
    async fn errors_when_device_credentials_unset() {
        let empty = DeviceCredential {
            device_id: String::new(),
            device_secret: String::new(),
        };
        let err = mint_device_token_for_tenant(
            "https://auth.example",
            &empty,
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("device_id"));
    }

    #[tokio::test]
    async fn mints_a_token_on_success_when_tenant_matches() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/token"))
            .and(body_json(serde_json::json!({
                "device_id": "device-1",
                "device_secret": "secret-1",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake.jwt.token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "tenant_id": "tenant-1",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let token = mint_device_token_for_tenant(
            &server.uri(),
            &credential(),
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(token, "fake.jwt.token");
    }

    #[tokio::test]
    async fn errors_when_response_tenant_mismatches_expected_tenant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake.jwt.token",
                "tenant_id": "tenant-WRONG",
            })))
            .mount(&server)
            .await;

        let err = mint_device_token_for_tenant(
            &server.uri(),
            &credential(),
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("tenant mismatch"));
        assert!(err.contains("tenant-1"));
        assert!(err.contains("tenant-WRONG"));
    }

    #[tokio::test]
    async fn propagates_4xx_as_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/token"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "invalid_credential",
            })))
            .mount(&server)
            .await;

        let err = mint_device_token_for_tenant(
            &server.uri(),
            &credential(),
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("401"));
    }

    #[tokio::test]
    async fn errors_on_malformed_response_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let err = mint_device_token_for_tenant(
            &server.uri(),
            &credential(),
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("Failed to parse"));
    }

    #[tokio::test]
    async fn errors_when_endpoint_unreachable() {
        // 未起動ポートに向けて接続エラーを起こす。
        let err = mint_device_token_for_tenant(
            "http://127.0.0.1:1",
            &credential(),
            "tenant-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("request failed"));
    }
}
