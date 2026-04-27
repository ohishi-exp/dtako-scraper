pub mod browser;
pub mod download;
pub mod login;
pub mod upload;

use tokio::sync::mpsc;
use tracing::{error, info};

use crate::config::Account;
use crate::error::ScraperError;

/// 進捗イベント
#[derive(Clone, serde::Serialize)]
pub struct ProgressEvent {
    pub event: String,    // "progress" or "result"
    pub comp_id: String,
    pub step: String,     // "login", "download", "upload", "done"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 1企業分のスクレイピング実行
pub async fn scrape(
    account: &Account,
    start_date: &str,
    end_date: &str,
    download_dir: &str,
    daiun_salary_url: &str,
    skip_upload: bool,
    progress_tx: Option<&mpsc::Sender<ProgressEvent>>,
) -> Result<String, ScraperError> {
    info!(
        "Starting scrape: comp_id={}, dates={} to {}",
        account.comp_id, start_date, end_date
    );

    let comp_id = &account.comp_id;

    // 企業ごと + 呼び出しごとにユニークなディレクトリを使う（並列 /scrape 時の race 防止）
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let account_dir = format!("{}/{}-{}", download_dir, comp_id, unique);

    let session = browser::BrowserSession::new(&account_dir).await?;

    // ログイン
    send_progress(progress_tx, comp_id, "login").await;
    login::login(&session.page, account).await?;

    // CSV ダウンロード
    send_progress(progress_tx, comp_id, "download").await;
    let zip_path =
        download::download_csv(&session.page, session.download_dir(), start_date, end_date)
            .await?;

    // ZIP の中身を一覧ログ（KUDGIVT.csv 欠落調査用）
    if let Ok(file) = std::fs::File::open(&zip_path) {
        if let Ok(mut archive) = zip::ZipArchive::new(file) {
            let mut entries: Vec<String> = Vec::new();
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    entries.push(format!("{} ({} bytes)", entry.name(), entry.size()));
                }
            }
            info!(
                "ZIP contents for comp_id={}: {} files: [{}]",
                comp_id,
                archive.len(),
                entries.join(", ")
            );
        } else {
            error!("Failed to open ZIP archive: {:?}", zip_path);
        }
    } else {
        error!("Failed to open ZIP file: {:?}", zip_path);
    }

    let result = if skip_upload {
        info!("skip_upload=true, skipping upload. ZIP at: {}", zip_path.display());
        format!("Download only. ZIP: {}", zip_path.display())
    } else {
        // daiun-salary にアップロード
        send_progress(progress_tx, comp_id, "upload").await;
        let uploaded = upload::upload_zip(daiun_salary_url, &account.tenant_id, &zip_path).await?;
        // クリーンアップ
        let _ = std::fs::remove_dir_all(&account_dir);
        uploaded
    };

    // ブラウザを閉じる
    if let Err(e) = session.page.close().await {
        error!("Failed to close page: {}", e);
    }

    info!("Scrape completed for comp_id={}", comp_id);
    Ok(result)
}

async fn send_progress(tx: Option<&mpsc::Sender<ProgressEvent>>, comp_id: &str, step: &str) {
    if let Some(tx) = tx {
        let _ = tx
            .send(ProgressEvent {
                event: "progress".into(),
                comp_id: comp_id.into(),
                step: step.into(),
                status: None,
                message: None,
            })
            .await;
    }
}
