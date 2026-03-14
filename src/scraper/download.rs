use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::Page;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::error::ScraperError;

const GENERAL_CSV_URL: &str = "https://theearth-np.com/F-NOS3010[GeneralCsv].aspx";
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// F-NOS3010[GeneralCsv].aspx から運行データ選択モード(rdoSelect0)で
/// 全選択 → csvdata.zip をダウンロード
///
/// 注意: rdoSelect1（日付範囲指定）モードはASP.NET ViewState同期の問題で
/// 日付入力後のダウンロードが動作しないため、rdoSelect0を使用する。
pub async fn download_csv(
    page: &Page,
    download_dir: &PathBuf,
    _start_date: &str, // 将来の日付フィルタリング用（現在未使用）
    _end_date: &str,
) -> Result<PathBuf, ScraperError> {
    info!("Navigating to GeneralCsv page...");

    // ダウンロード前の既存ファイル一覧
    let existing_files = get_existing_files(download_dir);

    page.goto(GENERAL_CSV_URL)
        .await
        .map_err(|e| ScraperError::Navigation(e.to_string()))?;

    sleep(Duration::from_secs(3)).await;

    // ページ確認（rdoSelect0 がデフォルトで選択されている）
    let page_check = page
        .evaluate(
            r#"(function() {
        var rdoSelect0 = document.querySelector('#rdoSelect0');
        var btnSelectAll = document.querySelector('#btnSelectAll');
        var btnCsv = document.querySelector('#btnCsv');
        return JSON.stringify({
            title: document.title,
            rdoSelect0: rdoSelect0 ? rdoSelect0.checked : null,
            btnSelectAll: !!btnSelectAll,
            btnCsv: !!btnCsv,
            url: location.href
        });
    })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("CSV page check: {:?}", page_check.into_value::<String>());

    // 全選択ボタンをクリック（ASP.NET postback で全行が選択される）
    info!("Clicking select-all button...");
    let select_result = page
        .evaluate(
            r#"(function() {
        var btn = document.querySelector('#btnSelectAll');
        if (btn) {
            btn.click();
            return JSON.stringify({ clicked: true, id: btn.id });
        }
        return JSON.stringify({ error: 'btnSelectAll not found' });
    })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!(
        "Select-all click: {:?}",
        select_result.into_value::<String>()
    );

    // 全選択の postback 完了を待機
    sleep(Duration::from_secs(5)).await;

    // 選択状態の確認
    let after_select = page
        .evaluate(
            r#"(function() {
        var items = document.querySelectorAll("span[id*='lblDisplayName_']");
        var count = 0;
        items.forEach(function(item) {
            var rect = item.getBoundingClientRect();
            if (rect.width > 0) count++;
        });
        return JSON.stringify({ visibleItems: count, url: location.href });
    })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!(
        "After select-all: {:?}",
        after_select.into_value::<String>()
    );

    // CSVダウンロードボタンクリック
    let csv_result = page
        .evaluate(
            r#"(function() {
        var btnCsv = document.querySelector('#btnCsv');
        if (btnCsv) {
            btnCsv.click();
            return JSON.stringify({ clicked: true, id: btnCsv.id, tag: btnCsv.tagName });
        }
        return JSON.stringify({ error: 'btnCsv not found' });
    })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!(
        "CSV download click: {:?}",
        csv_result.into_value::<String>()
    );

    // ダウンロード完了を待機
    let zip_path = wait_for_download(download_dir, &existing_files).await?;

    info!("Downloaded: {:?}", zip_path);
    Ok(zip_path)
}

fn get_existing_files(dir: &PathBuf) -> HashSet<PathBuf> {
    if !dir.exists() {
        return HashSet::new();
    }
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default()
}

async fn wait_for_download(
    download_dir: &PathBuf,
    existing_files: &HashSet<PathBuf>,
) -> Result<PathBuf, ScraperError> {
    let timeout = Duration::from_secs(DOWNLOAD_TIMEOUT_SECS);
    let poll_interval = Duration::from_millis(500);
    let start = std::time::Instant::now();

    loop {
        if let Ok(entries) = std::fs::read_dir(download_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();

                if existing_files.contains(&path) {
                    continue;
                }

                let filename = path.file_name().unwrap_or_default().to_string_lossy();

                // ダウンロード中はスキップ
                if filename.ends_with(".crdownload") || filename.ends_with(".tmp") {
                    debug!("Downloading: {}", filename);
                    continue;
                }

                // ZIP ファイル検出
                if let Some(ext) = path.extension() {
                    if ext.to_ascii_lowercase() == "zip" {
                        info!("ZIP file detected: {:?}", path);
                        return Ok(path);
                    }
                }

                // 拡張子なし（GUID形式）ファイル → .zip にリネーム
                if path.extension().is_none() {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if metadata.len() > 22 {
                            let zip_path = path.with_extension("zip");
                            if std::fs::rename(&path, &zip_path).is_ok() {
                                info!("Renamed GUID file to: {:?}", zip_path);
                                return Ok(zip_path);
                            }
                        }
                    }
                }
            }
        }

        if start.elapsed() > timeout {
            return Err(ScraperError::Timeout(format!(
                "Download did not complete within {}s",
                DOWNLOAD_TIMEOUT_SECS
            )));
        }

        tokio::time::sleep(poll_interval).await;
    }
}
