mod config;
mod error;
mod scraper;

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};
use chrono::Local;
use serde::{Deserialize, Serialize};
use tracing::info;

use config::AppConfig;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
}

#[derive(Deserialize)]
struct ScrapeRequest {
    /// 省略時は前日
    start_date: Option<String>, // "YYYY-MM-DD"
    /// 省略時は前日
    end_date: Option<String>,   // "YYYY-MM-DD"
    /// 特定の企業CDのみ実行（省略時は全企業）
    comp_id: Option<String>,
    /// アップロードをスキップ（テスト用）
    #[serde(default)]
    skip_upload: bool,
}

#[derive(Serialize)]
struct ScrapeResponse {
    results: Vec<ScrapeResult>,
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

async fn scrape_handler(
    State(state): State<AppState>,
    Json(req): Json<ScrapeRequest>,
) -> Result<Json<ScrapeResponse>, (StatusCode, String)> {
    let accounts: Vec<_> = if let Some(ref comp_id) = req.comp_id {
        state
            .config
            .accounts
            .iter()
            .filter(|a| a.comp_id == *comp_id)
            .collect()
    } else {
        state.config.accounts.iter().collect()
    };

    if accounts.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No matching accounts".into()));
    }

    let mut results = Vec::new();

    let yesterday = (Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let start_date = req.start_date.as_deref().unwrap_or(&yesterday);
    let end_date = req.end_date.as_deref().unwrap_or(&yesterday);

    for account in accounts {
        let result = scraper::scrape(
            account,
            start_date,
            end_date,
            &state.config.download_dir,
            &state.config.daiun_salary_url,
            req.skip_upload,
        )
        .await;

        results.push(match result {
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
        });
    }

    Ok(Json(ScrapeResponse { results }))
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
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/scrape", post(scrape_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind");

    info!("Listening on 0.0.0.0:{}", port);
    axum::serve(listener, app).await.expect("Server error");
}
