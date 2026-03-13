use serde::Deserialize;

/// 企業アカウント設定
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub comp_id: String,
    pub user_name: String,
    pub user_pass: String,
    /// daiun-salary 側の tenant_id
    pub tenant_id: String,
}

/// アプリケーション設定
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 企業アカウント一覧（JSON配列）
    pub accounts: Vec<Account>,
    /// daiun-salary の内部 API URL
    pub daiun_salary_url: String,
    /// ダウンロードディレクトリ
    pub download_dir: String,
    /// サーバーポート
    pub port: u16,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let accounts_json =
            std::env::var("DTAKO_ACCOUNTS").expect("DTAKO_ACCOUNTS must be set (JSON array)");
        let accounts: Vec<Account> =
            serde_json::from_str(&accounts_json).expect("DTAKO_ACCOUNTS must be valid JSON array");

        let daiun_salary_url =
            std::env::var("DAIUN_SALARY_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let download_dir =
            std::env::var("DOWNLOAD_DIR").unwrap_or_else(|_| "/tmp/dtako-downloads".into());
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .unwrap_or(8080);

        Self {
            accounts,
            daiun_salary_url,
            download_dir,
            port,
        }
    }
}
