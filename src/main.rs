mod config;
mod device_auth;
mod error;
// NET780 生データパーサー (Refs #18)。パース結果のアップロード/API連携は後続 issue の
// スコープなので、現時点では main から呼ばれず dead_code 警告が出る。
#[allow(dead_code)]
mod net780;
mod notify;
mod scraper;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{FixedOffset, Utc};
use futures::stream::{Stream, StreamExt};
use futures::SinkExt;
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

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        accounts_count: state.config.accounts.len(),
    })
}

/// リクエストに合致する account を解決し、無ければ Err を返す
fn resolve_accounts(
    state: &AppState,
    comp_id: &Option<String>,
) -> Result<Vec<config::Account>, (StatusCode, String)> {
    let accounts: Vec<_> = if let Some(ref comp_id) = comp_id {
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

    Ok(accounts)
}

/// スクレイプジョブを spawn し、各イベントを JSON 文字列として受け取れる channel を返す。
/// SSE (`/scrape`) と WebSocket (`/scrape/ws`) の両ハンドラがこれを共有する
/// (プロトコル差はイベントの運び方だけで、中身の JSON は同一)。
fn spawn_scrape_job(state: AppState, req: ScrapeRequest) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>(32);

    let accounts = match resolve_accounts(&state, &req.comp_id) {
        Ok(a) => a,
        Err((_, msg)) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "event": "error",
                    "message": msg,
                })) {
                    let _ = tx.send(json).await;
                }
            });
            return rx;
        }
    };

    let jst = FixedOffset::east_opt(9 * 3600).unwrap();
    let yesterday = (Utc::now().with_timezone(&jst) - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let start_date = req.start_date.unwrap_or_else(|| yesterday.clone());
    let end_date = req.end_date.unwrap_or(yesterday);
    let skip_upload = req.skip_upload;
    let config = state.config.clone();
    let comp_locks = state.comp_locks.clone();

    tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = mpsc::channel::<ProgressEvent>(32);

        // 進捗イベントを JSON 文字列に変換して送信するタスク
        let tx_clone = tx.clone();
        let progress_forwarder = tokio::spawn(async move {
            while let Some(evt) = progress_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&evt) {
                    let _ = tx_clone.send(json).await;
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

            let device_credential = config.device_credentials.get(&account.tenant_id);
            let result = scraper::scrape(
                account,
                &start_date,
                &end_date,
                &config.download_dir,
                &config.auth_worker_url,
                device_credential,
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
                let _ = tx.send(json).await;
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
            .send(serde_json::json!({"event": "done"}).to_string())
            .await;
    });

    rx
}

async fn scrape_handler(
    State(state): State<AppState>,
    Json(req): Json<ScrapeRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = spawn_scrape_job(state, req);
    let stream = ReceiverStream::new(rx).map(|json| Ok(Event::default().data(json)));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// WebSocket 版 `/scrape` (front Worker 専用、Refs dtako-scraper#403修正 の続き)。
/// GET + upgrade のため、パラメータは query string で受ける (`ScrapeRequest` と同一 shape)。
/// イベントの JSON は SSE 版と完全に同一 (data フィールド無しでそのまま text frame に載せる)。
async fn scrape_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(req): Query<ScrapeRequest>,
) -> Response {
    ws.on_upgrade(move |socket| handle_scrape_ws(socket, state, req))
}

async fn handle_scrape_ws(mut socket: WebSocket, state: AppState, req: ScrapeRequest) {
    let mut rx = spawn_scrape_job(state, req);
    while let Some(json) = rx.recv().await {
        if socket.send(Message::Text(json.into())).await.is_err() {
            warn!("scrape ws: client disconnected");
            return;
        }
    }
    let _ = socket.close().await;
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
        .route("/scrape/ws", get(scrape_ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind");

    info!("Listening on 0.0.0.0:{}", port);
    axum::serve(listener, app).await.expect("Server error");
}
