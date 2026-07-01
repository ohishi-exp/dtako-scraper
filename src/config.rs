use std::collections::HashMap;

use serde::Deserialize;

/// 企業アカウント設定
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub comp_id: String,
    pub user_name: String,
    pub user_pass: String,
    /// rust-alc-api 側の tenant_id (device credential のルックアップキーにもなる)
    pub tenant_id: String,
}

/// auth-worker device credential (tenant_id ごとに 1 組)。
/// pairing は `.github/workflows/provision-device.yml` で行う。
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCredential {
    pub device_id: String,
    pub device_secret: String,
}

/// アプリケーション設定
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 企業アカウント一覧（JSON配列）
    pub accounts: Vec<Account>,
    /// auth-worker の URL (`/device/token` + `/device-data-proxy/*` を叩く)
    pub auth_worker_url: String,
    /// tenant_id -> device credential。rust-alc-api への upload はこの経由でのみ行う
    /// (Cloud Run IAM lockdown 下では直接 HTTP は通らない、rust-alc-api#434)
    pub device_credentials: HashMap<String, DeviceCredential>,
    /// ダウンロードディレクトリ
    pub download_dir: String,
    /// サーバーポート
    pub port: u16,
    /// メール通知設定（環境変数未設定なら None）
    pub mail: Option<MailConfig>,
}

#[derive(Debug, Clone)]
pub struct MailConfig {
    pub smtp_user: String,
    pub smtp_pass: String,
    pub to: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let accounts_json =
            std::env::var("DTAKO_ACCOUNTS").expect("DTAKO_ACCOUNTS must be set (JSON array)");
        let accounts: Vec<Account> =
            serde_json::from_str(&accounts_json).expect("DTAKO_ACCOUNTS must be valid JSON array");

        let auth_worker_url =
            std::env::var("AUTH_WORKER_URL").unwrap_or_else(|_| "https://auth.ippoan.org".into());
        let device_credentials: HashMap<String, DeviceCredential> =
            match std::env::var("DTAKO_DEVICE_CREDENTIALS") {
                Ok(json) if !json.is_empty() => serde_json::from_str(&json)
                    .expect("DTAKO_DEVICE_CREDENTIALS must be a valid JSON object"),
                _ => HashMap::new(),
            };
        let download_dir =
            std::env::var("DOWNLOAD_DIR").unwrap_or_else(|_| "/tmp/dtako-downloads".into());
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .unwrap_or(8080);

        let mail = match (std::env::var("SMTP_USER"), std::env::var("SMTP_PASS")) {
            (Ok(smtp_user), Ok(smtp_pass)) => {
                let to = std::env::var("MAIL_TO").unwrap_or_else(|_| smtp_user.clone());
                Some(MailConfig {
                    smtp_user,
                    smtp_pass,
                    to,
                })
            }
            _ => None,
        };

        Self {
            accounts,
            auth_worker_url,
            device_credentials,
            download_dir,
            port,
            mail,
        }
    }
}
