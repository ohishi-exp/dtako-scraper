use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::Page;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::error::ScraperError;

const GENERAL_CSV_URL: &str = "https://theearth-np.com/F-NOS3010[GeneralCsv].aspx";
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// F-NOS3010[GeneralCsv].aspx から日付範囲指定で csvdata.zip をダウンロード
pub async fn download_csv(
    page: &Page,
    download_dir: &PathBuf,
    start_date: &str, // "YYYY-MM-DD"
    end_date: &str,   // "YYYY-MM-DD"
) -> Result<PathBuf, ScraperError> {
    info!("Navigating to GeneralCsv page...");

    // ダウンロード前の既存ファイル一覧
    let existing_files = get_existing_files(download_dir);

    page.goto(GENERAL_CSV_URL)
        .await
        .map_err(|e| ScraperError::Navigation(e.to_string()))?;

    sleep(Duration::from_secs(3)).await;

    // 日付範囲指定ラジオボタンを選択
    page.evaluate(
        r#"
        const radio = document.querySelector('#rdoSelect1');
        if (radio) { radio.click(); }
    "#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    sleep(Duration::from_secs(1)).await;

    // 日付をパース
    let (sy, sm, sd) = parse_date(start_date)?;
    let (ey, em, ed) = parse_date(end_date)?;

    // 日付範囲を入力
    let date_script = format!(
        r#"
        // 開始日
        var sy = document.querySelector('#ucStartDate1_txtYear');
        var sm = document.querySelector('#ucStartDate1_txtMonth');
        var sd = document.querySelector('#ucStartDate1_txtDay');
        if (sy) {{ sy.value = '{}'; }}
        if (sm) {{ sm.value = '{}'; }}
        if (sd) {{ sd.value = '{}'; }}
        // 終了日
        var ey = document.querySelector('#ucEndDate1_txtYear');
        var em = document.querySelector('#ucEndDate1_txtMonth');
        var ed = document.querySelector('#ucEndDate1_txtDay');
        if (ey) {{ ey.value = '{}'; }}
        if (em) {{ em.value = '{}'; }}
        if (ed) {{ ed.value = '{}'; }}
    "#,
        sy, sm, sd, ey, em, ed
    );

    page.evaluate(date_script.as_str())
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    sleep(Duration::from_secs(1)).await;

    // 全選択してCSVダウンロード
    // まず検索実行
    page.evaluate(
        r#"
        var btnSearch = document.querySelector('#btnLinkCsv') || document.querySelector('#btnSearch');
        if (btnSearch) { btnSearch.click(); }
    "#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    sleep(Duration::from_secs(5)).await;

    // リスト内の全項目を選択
    page.evaluate(
        r#"
        // 全てのリスト項目を選択
        var items = document.querySelectorAll("span[id*='lblDisplayName_']");
        items.forEach(function(item) {
            var row = item.closest('tr');
            if (row) { row.click(); }
        });
    "#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    sleep(Duration::from_secs(1)).await;

    // CSVダウンロードボタンクリック
    page.evaluate(
        r#"
        var btnCsv = document.querySelector('#btnCsv');
        if (btnCsv) { btnCsv.click(); }
    "#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    // ダウンロード完了を待機
    let zip_path = wait_for_download(download_dir, &existing_files).await?;

    info!("Downloaded: {:?}", zip_path);
    Ok(zip_path)
}

fn parse_date(date_str: &str) -> Result<(String, String, String), ScraperError> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return Err(ScraperError::Download(format!(
            "Invalid date format: {date_str}, expected YYYY-MM-DD"
        )));
    }
    // 年は下2桁
    let year = parts[0];
    let year_short = if year.len() == 4 {
        &year[2..]
    } else {
        year
    };
    Ok((
        year_short.to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
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

                // 拡張子なし（GUID形式）で十分なサイズ
                if path.extension().is_none() {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if metadata.len() > 100 {
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
