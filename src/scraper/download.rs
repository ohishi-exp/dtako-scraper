use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::Page;
use tokio::time::sleep;
use tracing::{debug, info};

use chrono::Datelike;

use crate::error::ScraperError;

const GENERAL_CSV_URL: &str = "https://theearth-np.com/F-NOS3010[GeneralCsv].aspx";
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// F-NOS3010[GeneralCsv].aspx から運行データ選択モード(rdoSelect1)で
/// 日付範囲指定 + 読取日指定 → csvdata.zip をダウンロード
///
/// 手順: rdoSelect1 クリック → rdoDate1（読取日指定）クリック → 日付 type 入力 → btnCsv
/// 日付フィールドには type_str でキーストローク入力する
/// （evaluate で値を直接セットすると ASP.NET ViewState が同期しない）
pub async fn download_csv(
    page: &Page,
    download_dir: &PathBuf,
    start_date: &str,
    end_date: &str,
) -> Result<PathBuf, ScraperError> {
    info!(
        "Navigating to GeneralCsv page... (dates: {} to {})",
        start_date, end_date
    );

    // ダウンロード前の既存ファイル一覧
    let existing_files = get_existing_files(download_dir);

    page.goto(GENERAL_CSV_URL)
        .await
        .map_err(|e| ScraperError::Navigation(e.to_string()))?;

    sleep(Duration::from_secs(3)).await;

    // テーブルの1行目から和暦/西暦を判定
    let is_wareki = detect_wareki(page).await;
    info!("Date format: {}", if is_wareki { "和暦(令和)" } else { "西暦" });

    // 日付パース
    let (sy, sm, sd) = parse_date_parts(start_date, is_wareki)?;
    let (ey, em, ed) = parse_date_parts(end_date, is_wareki)?;

    // rdoSelect1（日付範囲指定）をクリック（JS経由 — ラジオボタンが非表示の場合あり）
    info!("Clicking rdoSelect1 (date range mode)...");
    page.evaluate(
        r#"(function() {
        var r1 = document.querySelector('#rdoSelect1');
        if (r1) r1.click();
    })()"#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(format!("rdoSelect1 click failed: {e}")))?;

    // postback 完了待ち
    sleep(Duration::from_secs(3)).await;

    // rdoDate1（読取日指定）をクリック
    info!("Clicking rdoDate1 (reading date mode)...");
    page.evaluate(
        r#"(function() {
        var r = document.querySelector('#rdoDate1');
        if (r) r.click();
    })()"#,
    )
    .await
    .map_err(|e| ScraperError::JavaScript(format!("rdoDate1 click failed: {e}")))?;

    sleep(Duration::from_secs(1)).await;

    // 日付フィールドに type_str で入力
    info!(
        "Typing date range: {}/{}/{} - {}/{}/{}",
        sy, sm, sd, ey, em, ed
    );

    let date_fields = [
        ("#MainContent_ucStartDate_txtYear", &sy),
        ("#MainContent_ucStartDate_txtMonth", &sm),
        ("#MainContent_ucStartDate_txtDay", &sd),
        ("#MainContent_ucEndDate_txtYear", &ey),
        ("#MainContent_ucEndDate_txtMonth", &em),
        ("#MainContent_ucEndDate_txtDay", &ed),
    ];

    for (selector, value) in date_fields {
        // JS でフィールドをクリア
        page.evaluate(format!(
            r#"(function() {{
            var el = document.querySelector('{}');
            if (el) {{ el.value = ''; }}
        }})()"#,
            selector
        ))
        .await
        .map_err(|e| ScraperError::JavaScript(format!("{} clear failed: {e}", selector)))?;

        // find_element → click → type_str でキーストローク入力
        let el = page
            .find_element(selector)
            .await
            .map_err(|e| ScraperError::JavaScript(format!("{} not found: {e}", selector)))?;
        el.click()
            .await
            .map_err(|e| ScraperError::JavaScript(format!("{} click failed: {e}", selector)))?;
        el.type_str(value)
            .await
            .map_err(|e| ScraperError::JavaScript(format!("{} type failed: {e}", selector)))?;
        debug!("  {} = {}", selector, value);
    }

    sleep(Duration::from_millis(500)).await;

    // 入力後の実際の値をフィールドから読み戻す（typing が ASP.NET postback で書き換えられていないか確認）
    let actual_values = page
        .evaluate(
            r#"(function() {
        var ids = [
            'MainContent_ucStartDate_txtYear',
            'MainContent_ucStartDate_txtMonth',
            'MainContent_ucStartDate_txtDay',
            'MainContent_ucEndDate_txtYear',
            'MainContent_ucEndDate_txtMonth',
            'MainContent_ucEndDate_txtDay'
        ];
        return JSON.stringify(ids.map(function(id){
            var el = document.getElementById(id);
            return id + '=' + (el ? el.value : 'NOT_FOUND');
        }));
    })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(format!("readback failed: {e}")))?;
    info!(
        "Actual date field values after typing: {:?}",
        actual_values.into_value::<String>()
    );

    // CSVダウンロードボタンクリック
    info!("Clicking btnCsv...");
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

/// テーブルの1行目の日付(YY/MM/DD)から和暦か西暦かを判定
async fn detect_wareki(page: &Page) -> bool {
    let result = page
        .evaluate(
            r#"(function() {
        var tds = document.querySelectorAll('td');
        for (var i = 0; i < tds.length; i++) {
            var t = tds[i].textContent.trim();
            if (/^\d{2}\/\d{2}\/\d{2}$/.test(t)) return t;
        }
        return null;
    })()"#,
        )
        .await;

    match result {
        Ok(val) => {
            if let Some(date_str) = val.into_value::<Option<String>>().ok().flatten() {
                if let Some(yy_str) = date_str.split('/').next() {
                    if let Ok(page_year) = yy_str.parse::<i32>() {
                        let now_year = chrono::Utc::now().year();
                        let western_yy = now_year % 100;
                        let reiwa_yy = now_year - 2018;
                        let is_wareki =
                            (page_year - reiwa_yy).abs() < (page_year - western_yy).abs();
                        info!(
                            "Date detection: first_date={}, page_year={}, reiwa={}, western={} → {}",
                            date_str,
                            page_year,
                            reiwa_yy,
                            western_yy,
                            if is_wareki { "和暦" } else { "西暦" }
                        );
                        return is_wareki;
                    }
                }
            }
            info!("No date found in table, defaulting to 和暦");
            true
        }
        Err(_) => {
            info!("Failed to detect date format, defaulting to 和暦");
            true
        }
    }
}

/// "YYYY-MM-DD" → (年2桁, 月2桁, 日2桁) にパース
/// is_wareki=true の場合、西暦→令和に変換 (2026 → 08)
fn parse_date_parts(
    date: &str,
    is_wareki: bool,
) -> Result<(String, String, String), ScraperError> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return Err(ScraperError::Download(format!(
            "Invalid date format '{}', expected YYYY-MM-DD",
            date
        )));
    }
    let year: i32 = parts[0]
        .parse()
        .map_err(|_| ScraperError::Download(format!("Invalid year in '{}'", date)))?;

    let yy = if is_wareki {
        // 西暦→令和: 2026 → 8
        let reiwa = year - 2018;
        format!("{:02}", reiwa)
    } else {
        // 西暦下2桁: 2026 → 26
        format!("{:02}", year % 100)
    };

    Ok((yy, parts[1].to_string(), parts[2].to_string()))
}

pub(crate) fn get_existing_files(dir: &PathBuf) -> HashSet<PathBuf> {
    if !dir.exists() {
        return HashSet::new();
    }
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default()
}

pub(crate) async fn wait_for_download(
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
