mod config;
mod error;
mod notify;
mod scraper;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use chrono::{FixedOffset, Utc};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, warn};

use config::AppConfig;
use scraper::ProgressEvent;

/// comp_id 別の排他ロック (同一企業への並列 scrape を直列化し、source 側のセッション衝突を防ぐ)
type CompLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    comp_locks: CompLocks,
}

async fn comp_lock(locks: &CompLocks, comp_id: &str) -> Arc<Mutex<()>> {
    let mut map = locks.lock().await;
    map.entry(comp_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[derive(Deserialize)]
struct ScrapeRequest {
    /// 省略時は前日
    start_date: Option<String>, // "YYYY-MM-DD"
    /// 省略時は前日
    end_date: Option<String>, // "YYYY-MM-DD"
    /// 特定の企業CDのみ実行（省略時は全企業）
    comp_id: Option<String>,
    /// アップロードをスキップ（テスト用）
    #[serde(default)]
    skip_upload: bool,
}

#[derive(Serialize)]
struct ScrapeResult {
    comp_id: String,
    status: String,
    message: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    accounts_count: usize,
}

/// F-VOS3020 車輌設定 ZIP 取得リクエスト (email-receiver 用)。
#[derive(Deserialize)]
struct VehicleSettingRequest {
    /// 省略時は全企業を順に試す。
    comp_id: Option<String>,
    /// 例: "(16) 十勝800か16"
    vehicle_name: String,
    /// RFC3339。この時刻に最も近い運行を選ぶ。
    received_at: String,
    /// true なら ZIP を base64 で返す (size <= 5MB)。既定 true。
    #[serde(default = "default_true")]
    skip_upload: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct VehicleSettingResponse {
    comp_id: String,
    unko_no: String,
    vehicle_name: String,
    operation_started_at: Option<String>,
    operation_ended_at: Option<String>,
    zip_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    zip_base64: Option<String>,
}

/// base64 に載せる ZIP の上限 (issue #5: skip_upload=true 時 size <= 5MB)。
const MAX_BASE64_ZIP_BYTES: u64 = 5 * 1024 * 1024;

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        accounts_count: state.config.accounts.len(),
    })
}

async fn scrape_handler(
    State(state): State<AppState>,
    Json(req): Json<ScrapeRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let accounts: Vec<_> = if let Some(ref comp_id) = req.comp_id {
        state
            .config
            .accounts
            .iter()
            .filter(|a| a.comp_id == *comp_id)
            .cloned()
            .collect()
    } else {
        state.config.accounts.clone()
    };

    if accounts.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No matching accounts".into()));
    }

    let jst = FixedOffset::east_opt(9 * 3600).unwrap();
    let yesterday = (Utc::now().with_timezone(&jst) - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let start_date = req.start_date.unwrap_or_else(|| yesterday.clone());
    let end_date = req.end_date.unwrap_or(yesterday);
    let skip_upload = req.skip_upload;
    let config = state.config.clone();
    let comp_locks = state.comp_locks.clone();

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = mpsc::channel::<ProgressEvent>(32);

        // 進捗イベントを SSE に変換して送信するタスク
        let tx_clone = tx.clone();
        let progress_forwarder = tokio::spawn(async move {
            while let Some(evt) = progress_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&evt) {
                    let _ = tx_clone.send(Ok(Event::default().data(json))).await;
                }
            }
        });

        let mut results = Vec::new();

        for account in &accounts {
            // 同一 comp_id の並列 scrape を直列化（source 側 session / ダウンロードディレクトリ race の予防）
            let lock = comp_lock(&comp_locks, &account.comp_id).await;
            let acquire = lock.try_lock();
            let _guard = match acquire {
                Ok(g) => g,
                Err(_) => {
                    warn!(
                        "comp_id={} is currently scraping in another call; waiting for lock...",
                        account.comp_id
                    );
                    lock.lock().await
                }
            };

            let result = scraper::scrape(
                account,
                &start_date,
                &end_date,
                &config.download_dir,
                &config.daiun_salary_url,
                skip_upload,
                Some(&progress_tx),
            )
            .await;

            let scrape_result = match result {
                Ok(msg) => ScrapeResult {
                    comp_id: account.comp_id.clone(),
                    status: "success".into(),
                    message: msg,
                },
                Err(e) => ScrapeResult {
                    comp_id: account.comp_id.clone(),
                    status: "error".into(),
                    message: e.to_string(),
                },
            };

            // 企業ごとの結果イベント
            if let Ok(json) = serde_json::to_string(&ProgressEvent {
                event: "result".into(),
                comp_id: scrape_result.comp_id.clone(),
                step: "done".into(),
                status: Some(scrape_result.status.clone()),
                message: Some(scrape_result.message.clone()),
            }) {
                let _ = tx.send(Ok(Event::default().data(json))).await;
            }

            results.push(scrape_result);
        }

        // メール通知
        if let Some(ref mail_config) = config.mail {
            let has_error = results.iter().any(|r| r.status == "error");
            let subject = if has_error {
                format!(
                    "[dtako-scraper] ⚠ エラーあり ({} ~ {})",
                    start_date, end_date
                )
            } else {
                format!("[dtako-scraper] ✅ 成功 ({} ~ {})", start_date, end_date)
            };
            let body = results
                .iter()
                .map(|r| format!("[{}] {} - {}", r.status, r.comp_id, r.message))
                .collect::<Vec<_>>()
                .join("\n");
            notify::send_result_mail(mail_config, &subject, &body).await;
        }

        // progress チャネルを閉じて forwarder を終了
        drop(progress_tx);
        let _ = progress_forwarder.await;

        // 完了イベント
        let _ = tx
            .send(Ok(
                Event::default().data(serde_json::json!({"event": "done"}).to_string())
            ))
            .await;
    });

    let stream = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `SCRAPER_API_KEY` env が設定されていれば `X-Scraper-API-Key` ヘッダを
/// constant-time 比較で検証する。未設定なら認証なし (既存 `/scrape` と同方針、
/// ingress 制限は後追い。Refs issue #5)。
fn verify_scraper_api_key(headers: &axum::http::HeaderMap) -> Result<(), (StatusCode, String)> {
    let expected = match std::env::var("SCRAPER_API_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(()), // 未設定 = 認証スキップ
    };
    let provided = headers
        .get("X-Scraper-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "invalid X-Scraper-API-Key".into()))
    }
}

/// timing-safe な byte 列等値比較 (短絡しない)。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max = a.len().max(b.len());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..max {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

async fn vehicle_setting_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<VehicleSettingRequest>,
) -> Result<Json<VehicleSettingResponse>, (StatusCode, String)> {
    verify_scraper_api_key(&headers)?;

    if req.vehicle_name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "vehicle_name is required".into()));
    }

    // comp_id 指定があればその企業のみ、無ければ全企業を順に試す。
    let candidates: Vec<_> = if let Some(ref cid) = req.comp_id {
        state
            .config
            .accounts
            .iter()
            .filter(|a| a.comp_id == *cid)
            .cloned()
            .collect()
    } else {
        state.config.accounts.clone()
    };
    if candidates.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No matching accounts".into()));
    }

    let download_dir = state.config.download_dir.clone();
    let mut last_err: Option<String> = None;

    for account in &candidates {
        // 同一 comp_id の並列実行を直列化 (source 側 session 衝突防止)。
        let lock = comp_lock(&state.comp_locks, &account.comp_id).await;
        let _guard = lock.lock().await;

        match scraper::scrape_vehicle_setting(
            account,
            &req.vehicle_name,
            &req.received_at,
            &download_dir,
        )
        .await
        {
            Ok(result) => {
                let resp = build_vehicle_setting_response(account, &result, req.skip_upload);
                // 取得した一時ファイルを片付け (base64 はメモリに載せ済み)。
                if let Some(parent) = result.zip_path.parent() {
                    let _ = std::fs::remove_dir_all(parent);
                }
                return resp.map(Json);
            }
            Err(e) => {
                warn!(
                    "vehicle-setting scrape failed for comp_id={}: {}",
                    account.comp_id, e
                );
                last_err = Some(format!("{}: {e}", account.comp_id));
            }
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        format!(
            "vehicle '{}' not found in any company. last_error: {}",
            req.vehicle_name,
            last_err.unwrap_or_else(|| "none".into())
        ),
    ))
}

fn build_vehicle_setting_response(
    account: &config::Account,
    result: &scraper::vehicle_setting::VehicleSettingResult,
    skip_upload: bool,
) -> Result<VehicleSettingResponse, (StatusCode, String)> {
    let bytes = std::fs::read(&result.zip_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read zip failed: {e}"),
        )
    })?;
    let size = bytes.len() as u64;

    let zip_base64 = if skip_upload && size <= MAX_BASE64_ZIP_BYTES {
        use base64::Engine;
        Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
    } else {
        if skip_upload {
            warn!(
                "zip size {} bytes exceeds base64 limit {}; omitting zip_base64",
                size, MAX_BASE64_ZIP_BYTES
            );
        }
        None
    };

    Ok(VehicleSettingResponse {
        comp_id: account.comp_id.clone(),
        unko_no: result.unko_no.clone(),
        vehicle_name: result.vehicle_name.clone(),
        operation_started_at: result.operation_started_at.clone(),
        operation_ended_at: result.operation_ended_at.clone(),
        zip_size_bytes: size,
        zip_base64,
    })
}

#[tokio::main]
async fn main() {
    // .env 読み込み
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dtako_scraper=info".into()),
        )
        .json()
        .init();

    let config = AppConfig::from_env();
    let port = config.port;

    info!(
        "Starting dtako-scraper on port {}, {} accounts configured",
        port,
        config.accounts.len()
    );

    let state = AppState {
        config: Arc::new(config),
        comp_locks: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/scrape", post(scrape_handler))
        .route("/scrape-vehicle-setting", post(vehicle_setting_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind");

    info!("Listening on 0.0.0.0:{}", port);
    axum::serve(listener, app).await.expect("Server error");
}
