//! F-VOS3020[VehicleComDataDownLoad].aspx から、車輌 + メール受信日時に最も近い
//! 1 運行の設定 ZIP を取得する。
//!
//! 画面構造 (2026-06 ライブ調査、Refs ippoan/email-receiver#1):
//! - 運行一覧は ASP.NET ListView `lstOperation`。各データ行は
//!   `tr#MainContent_ucDataSelect_lstOperation_row_{N}` で、列 span は
//!   `lbl{Field}_{N}` (VehicleName / StartDateTime / OperationNo /
//!   OperationStartDateTime / OperationEndDateTime / OperationDate ...)。
//! - 行選択は checkbox ではなく `onclick="RowsClick(this)"` 方式。単一選択モードでは
//!   hidden field `txtOperationNo`(=cells[0]) / `txtStartDateTime`(=cells[1]) /
//!   `txtCurrentID`(=row id) / `txtIndex` に値を入れることで選択状態を表す。
//! - `#MainContent_btnPreview` (表示は「ダウンロード」) が選択運行の設定 ZIP を DL。
//! - 一覧は `[読取日]` でフィルタされる (既定は直近)。

use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::Page;
use chrono::{FixedOffset, NaiveDateTime, TimeZone};
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::error::ScraperError;
use crate::scraper::download::{get_existing_files, wait_for_download};
use crate::scraper::vehicle_match::vehicle_matches;

const FVOS3020_URL: &str = "https://theearth-np.com/F-VOS3020[VehicleComDataDownLoad].aspx";

/// F-VOS3020 の 1 運行行から抜き出すデータ。
#[derive(Debug, Clone, Deserialize)]
struct OperationRow {
    index: i64,
    row_id: String,
    vehicle_name: String,
    /// cells[1] (= RowsClick が txtStartDateTime に入れる値)。"2026/06/15 3:59:36"
    start_date_time: String,
    /// cells[0] (= 運行No、22桁)
    operation_no: String,
    operation_start_date_time: String,
    operation_end_date_time: String,
}

/// scrape_vehicle_setting の結果 (1 運行)。
#[derive(Debug, Clone)]
pub struct VehicleSettingResult {
    pub unko_no: String,
    pub vehicle_name: String,
    pub operation_started_at: Option<String>,
    pub operation_ended_at: Option<String>,
    pub zip_path: PathBuf,
}

/// F-VOS3020 で vehicle_name + received_at に最も近い運行の設定 ZIP を DL する。
///
/// 事前に `login()` 済みの page を渡すこと。
pub async fn download_vehicle_setting(
    page: &Page,
    download_dir: &PathBuf,
    vehicle_name: &str,
    received_at: &str,
) -> Result<VehicleSettingResult, ScraperError> {
    navigate_to_fvos3020(page).await?;

    // 一覧が描画されるまで待つ (postback / UpdatePanel)。
    let mut ready = false;
    for _ in 0..15 {
        let has_rows = page
            .evaluate(
                "document.querySelector('tr[id^=\"MainContent_ucDataSelect_lstOperation_row_\"]') !== null",
            )
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false);
        if has_rows {
            ready = true;
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }
    if !ready {
        return Err(ScraperError::Download(
            "F-VOS3020 operation list did not render (no lstOperation rows)".into(),
        ));
    }

    // 全行を JSON で読み出す。
    let rows = read_operation_rows(page).await?;
    info!(
        "F-VOS3020: {} operation rows; target vehicle='{}' received_at='{}'",
        rows.len(),
        vehicle_name,
        received_at
    );
    // 診断用に全行をログ (初回ライブ調整用、issue #5)。
    for r in &rows {
        info!(
            "  row[{}] vehicle='{}' start='{}' opno='{}'",
            r.index, r.vehicle_name, r.start_date_time, r.operation_no
        );
    }

    let chosen = pick_nearest(&rows, vehicle_name, received_at)?;
    info!(
        "F-VOS3020: chosen row[{}] vehicle='{}' opno='{}' start='{}'",
        chosen.index, chosen.vehicle_name, chosen.operation_no, chosen.start_date_time
    );

    // 行を選択 (RowsClick の単一選択パスと同じ hidden field を直接セット)。
    select_row(page, chosen).await?;

    // ダウンロードボタン押下 → ZIP DL。
    let existing = get_existing_files(download_dir);
    info!("Clicking btnPreview (download)...");
    let click = page
        .evaluate(
            r#"(function(){
            var b = document.getElementById('MainContent_btnPreview');
            if (!b) return JSON.stringify({error:'btnPreview not found'});
            b.click();
            return JSON.stringify({clicked:true});
        })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("btnPreview click: {:?}", click.into_value::<String>());

    let zip_path = wait_for_download(download_dir, &existing).await?;
    info!("Vehicle setting ZIP downloaded: {:?}", zip_path);

    Ok(VehicleSettingResult {
        unko_no: chosen.operation_no.clone(),
        vehicle_name: chosen.vehicle_name.clone(),
        operation_started_at: non_empty(&chosen.operation_start_date_time),
        operation_ended_at: non_empty(&chosen.operation_end_date_time),
        zip_path,
    })
}

/// メニューボタン経路で F-VOS3020 に到達する (issue #5: Button2nd_4 → Button3rd_2)。
/// 直 URL 遷移は headless で不安定なため、ボタン経路を主とし、一覧が出なければ
/// 直 navigate を fallback にする。
async fn navigate_to_fvos3020(page: &Page) -> Result<(), ScraperError> {
    info!("Navigating to F-VOS3020 via menu buttons...");
    let clicked = page
        .evaluate(
            r#"(function(){
            function clickByName(name){
                var el = document.querySelector('[name="'+name+'"]');
                if (el) { el.click(); return true; }
                return false;
            }
            var r1 = clickByName('ctl00$MainContent$Button2nd_4');
            return JSON.stringify({button2nd_4: r1});
        })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("Button2nd_4 click: {:?}", clicked.into_value::<String>());
    sleep(Duration::from_secs(2)).await;

    let clicked3 = page
        .evaluate(
            r#"(function(){
            var el = document.querySelector('[name="ctl00$MainContent$Button3rd_2"]');
            if (el) { el.click(); return JSON.stringify({button3rd_2:true}); }
            return JSON.stringify({button3rd_2:false});
        })()"#,
        )
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("Button3rd_2 click: {:?}", clicked3.into_value::<String>());
    sleep(Duration::from_secs(3)).await;

    // 一覧が出ていなければ直 URL 遷移で fallback。
    let has_rows = page
        .evaluate(
            "document.querySelector('tr[id^=\"MainContent_ucDataSelect_lstOperation_row_\"]') !== null",
        )
        .await
        .ok()
        .and_then(|v| v.into_value::<bool>().ok())
        .unwrap_or(false);
    if !has_rows {
        warn!("Menu nav did not reach F-VOS3020 list; falling back to direct navigate");
        page.goto(FVOS3020_URL)
            .await
            .map_err(|e| ScraperError::Navigation(format!("F-VOS3020 navigate failed: {e}")))?;
        sleep(Duration::from_secs(3)).await;
    }
    Ok(())
}

/// 全運行行を JSON で読み出す。
async fn read_operation_rows(page: &Page) -> Result<Vec<OperationRow>, ScraperError> {
    let script = r#"(function(){
        var rows = document.querySelectorAll('tr[id^="MainContent_ucDataSelect_lstOperation_row_"]');
        function txt(prefix, idx){
            var el = document.getElementById('MainContent_ucDataSelect_lstOperation_'+prefix+'_'+idx);
            return el ? el.innerText.trim() : '';
        }
        var out = [];
        for (var i = 0; i < rows.length; i++) {
            var id = rows[i].id;
            var m = id.match(/_row_(\d+)$/);
            if (!m) continue;
            var idx = parseInt(m[1], 10);
            out.push({
                index: idx,
                row_id: id,
                vehicle_name: txt('lblVehicleName', idx),
                start_date_time: txt('lblStartDateTime', idx),
                operation_no: txt('lblOperationNo', idx),
                operation_start_date_time: txt('lblOperationStartDateTime', idx),
                operation_end_date_time: txt('lblOperationEndDateTime', idx)
            });
        }
        return JSON.stringify(out);
    })()"#;
    let result = page
        .evaluate(script)
        .await
        .map_err(|e| ScraperError::JavaScript(format!("read rows failed: {e}")))?;
    let json = result
        .into_value::<String>()
        .map_err(|e| ScraperError::JavaScript(format!("read rows decode failed: {e}")))?;
    serde_json::from_str::<Vec<OperationRow>>(&json)
        .map_err(|e| ScraperError::Download(format!("parse rows failed: {e}; raw={json}")))
}

/// vehicle_name が一致する行のうち、received_at に最も近い start_date_time を選ぶ。
fn pick_nearest<'a>(
    rows: &'a [OperationRow],
    vehicle_name: &str,
    received_at: &str,
) -> Result<&'a OperationRow, ScraperError> {
    let target = parse_received_at_jst(received_at)?;

    let mut best: Option<(&OperationRow, i64)> = None;
    for r in rows {
        if !vehicle_matches(vehicle_name, &r.vehicle_name) {
            continue;
        }
        let diff = match parse_page_datetime_jst(&r.start_date_time) {
            Some(dt) => (dt - target).num_seconds().abs(),
            // 日時が読めない行は最後尾優先度 (巨大 diff)。
            None => i64::MAX,
        };
        match best {
            Some((_, bd)) if bd <= diff => {}
            _ => best = Some((r, diff)),
        }
    }

    best.map(|(r, _)| r).ok_or_else(|| {
        ScraperError::Download(format!(
            "no operation row matched vehicle '{vehicle_name}' (rows checked: {})",
            rows.len()
        ))
    })
}

/// 選んだ行を選択状態にする。RowsClick の単一選択パスと同じ hidden field を直接セットし、
/// 行の背景もハイライトする (視認・スクショ用)。
async fn select_row(page: &Page, row: &OperationRow) -> Result<(), ScraperError> {
    let script = format!(
        r#"(function(){{
            var rowId = {row_id};
            var idx = {idx};
            var opno = {opno};
            var startdt = {startdt};
            var el = document.getElementById(rowId);
            if (el) {{ el.style.backgroundColor = '#79abf7'; el.style.color = '#fff'; }}
            function setv(id, v){{ var e = document.getElementById(id); if (e) e.value = v; }}
            setv('txtOperationNo', opno);
            setv('txtStartDateTime', startdt);
            setv('txtCurrentID', rowId);
            setv('txtIndex', String(idx));
            return JSON.stringify({{ ok: !!el }});
        }})()"#,
        row_id = serde_json::to_string(&row.row_id).unwrap(),
        idx = row.index,
        opno = serde_json::to_string(&row.operation_no).unwrap(),
        startdt = serde_json::to_string(&row.start_date_time).unwrap(),
    );
    let r = page
        .evaluate(script.as_str())
        .await
        .map_err(|e| ScraperError::JavaScript(format!("select_row failed: {e}")))?;
    info!("select_row: {:?}", r.into_value::<String>());
    sleep(Duration::from_millis(500)).await;
    Ok(())
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid offset")
}

/// "2026/06/15 3:59:36" (JST wall clock) → JST DateTime。
fn parse_page_datetime_jst(s: &str) -> Option<chrono::DateTime<FixedOffset>> {
    let s = s.trim();
    for fmt in ["%Y/%m/%d %H:%M:%S", "%Y/%m/%d %H:%M"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return jst().from_local_datetime(&ndt).single();
        }
    }
    None
}

/// RFC3339 (例 "2026-06-15T08:00:00Z") を JST に変換。
fn parse_received_at_jst(s: &str) -> Result<chrono::DateTime<FixedOffset>, ScraperError> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&jst()))
        .map_err(|e| ScraperError::Download(format!("invalid received_at '{s}': {e}")))
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(idx: i64, vehicle: &str, start: &str, opno: &str) -> OperationRow {
        OperationRow {
            index: idx,
            row_id: format!("MainContent_ucDataSelect_lstOperation_row_{idx}"),
            vehicle_name: vehicle.into(),
            start_date_time: start.into(),
            operation_no: opno.into(),
            operation_start_date_time: start.into(),
            operation_end_date_time: String::new(),
        }
    }

    #[test]
    fn picks_nearest_same_vehicle() {
        let rows = vec![
            row(0, "十勝800か16", "2026/06/15 3:00:00", "A"),
            row(1, "十勝800か16", "2026/06/15 9:00:00", "B"),
            row(2, "札幌100あ17", "2026/06/15 8:05:00", "C"),
        ];
        // received_at 08:00 UTC = 17:00 JST → nearest same-vehicle is row1 (09:00 JST)
        let got = pick_nearest(&rows, "(16) 十勝800か16", "2026-06-15T08:00:00Z").unwrap();
        assert_eq!(got.operation_no, "B");
    }

    #[test]
    fn picks_nearest_when_received_is_jst_morning() {
        let rows = vec![
            row(0, "十勝800か16", "2026/06/15 3:00:00", "A"),
            row(1, "十勝800か16", "2026/06/15 9:00:00", "B"),
        ];
        // received_at 18:30 UTC prev day = 03:30 JST → nearest is row0 (03:00 JST)
        let got = pick_nearest(&rows, "十勝800か16", "2026-06-14T18:30:00Z").unwrap();
        assert_eq!(got.operation_no, "A");
    }

    #[test]
    fn errors_when_no_vehicle_match() {
        let rows = vec![row(0, "札幌100あ17", "2026/06/15 3:00:00", "A")];
        let err = pick_nearest(&rows, "(16) 十勝800か16", "2026-06-15T08:00:00Z").unwrap_err();
        assert!(matches!(err, ScraperError::Download(_)));
    }

    #[test]
    fn rows_with_unparseable_datetime_are_lowest_priority() {
        let rows = vec![
            row(0, "十勝800か16", "ぐちゃ", "A"),
            row(1, "十勝800か16", "2026/06/15 9:00:00", "B"),
        ];
        let got = pick_nearest(&rows, "十勝800か16", "2026-06-15T00:00:00Z").unwrap();
        assert_eq!(got.operation_no, "B");
    }

    #[test]
    fn page_datetime_parses_single_digit_hour() {
        assert!(parse_page_datetime_jst("2026/06/15 3:59:36").is_some());
        assert!(parse_page_datetime_jst("2026/06/15 13:59").is_some());
        assert!(parse_page_datetime_jst("bad").is_none());
    }

    #[test]
    fn received_at_rejects_garbage() {
        assert!(parse_received_at_jst("not-a-date").is_err());
        assert!(parse_received_at_jst("2026-06-15T08:00:00Z").is_ok());
    }
}
