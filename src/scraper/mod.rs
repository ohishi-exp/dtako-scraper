pub mod browser;
pub mod download;
pub mod login;
pub mod upload;

use tracing::{error, info};

use crate::config::Account;
use crate::error::ScraperError;

/// 1企業分のスクレイピング実行
pub async fn scrape(
    account: &Account,
    start_date: &str,
    end_date: &str,
    download_dir: &str,
    daiun_salary_url: &str,
) -> Result<String, ScraperError> {
    info!(
        "Starting scrape: comp_id={}, dates={} to {}",
        account.comp_id, start_date, end_date
    );

    // 企業ごとにダウンロードディレクトリを分離
    let account_dir = format!("{}/{}", download_dir, account.comp_id);
    // 古いファイルをクリーンアップ
    let _ = std::fs::remove_dir_all(&account_dir);

    let session = browser::BrowserSession::new(&account_dir).await?;

    // ログイン
    login::login(&session.page, account).await?;

    // CSV ダウンロード
    let zip_path =
        download::download_csv(&session.page, session.download_dir(), start_date, end_date)
            .await?;

    // daiun-salary にアップロード
    let result = upload::upload_zip(daiun_salary_url, &account.tenant_id, &zip_path).await?;

    // クリーンアップ
    let _ = std::fs::remove_dir_all(&account_dir);

    // ブラウザを閉じる
    if let Err(e) = session.page.close().await {
        error!("Failed to close page: {}", e);
    }

    info!("Scrape completed for comp_id={}", account.comp_id);
    Ok(result)
}
