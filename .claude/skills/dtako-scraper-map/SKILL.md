---
name: dtako-scraper-map
generated-from: dtako-scraper:40443c4a5dae3130bfce30342fe8e4a4d1969ee6
paths: [src/]
description: ohishi-exp/dtako-scraper (Rust + Axum + chromiumoxide ヘッドレス Chrome の Dtakolog CSV スクレイパー) の構造ナビゲーション。theearth-np.com から csvdata.zip を取得 → daiun-salary API に multipart upload する Cloud Run サービスの module 配置・SSE 進捗・運用 gotcha を 1 枚にまとめる。トリガー:「dtako-scraper」「Dtakolog」「csvdata.zip」「theearth-np」「chromiumoxide」「headless-shell」「KUDGIVT」「comp_id 並列」「daiun-salary upload」等。
---

# dtako-scraper-map — ohishi-exp/dtako-scraper 構造ナビゲーション

Rust + Axum 0.8 の単一バイナリ。chromiumoxide (CDP) でヘッドレス Chrome を操作して
theearth-np.com から Dtakolog の `csvdata.zip` を DL → daiun-salary API に multipart upload する。

> ここは索引。細部 (関数シグネチャ・正確な行) は repo 側が正。
> frontmatter の `generated-from` が現在の tree-sha とズレたら
> session-start-skill-coverage hook が再生成を促す → その時 tree-sha を更新する。

## 区画 (module)

| module | 主要ファイル | 役割 |
|---|---|---|
| **entrypoint** | `src/main.rs` | Axum router + `AppState` (config + comp_id 別ロック) + SSE handler |
| **config** | `src/config.rs` | `AppConfig` / `Account` (`DTAKO_ACCOUNTS` JSON をパース) |
| **scraper** | `src/scraper/mod.rs` | `scrape()` 1 企業分のフロー統括 + `ProgressEvent` 定義 |
| ├ browser | `src/scraper/browser.rs` | chromiumoxide で Chrome 起動・CDP 操作 |
| ├ login | `src/scraper/login.rs` | theearth-np.com ログイン |
| ├ download | `src/scraper/download.rs` | 日付指定で csvdata.zip DL (ZIP 内容ログ出力) |
| └ upload | `src/scraper/upload.rs` | reqwest multipart で daiun-salary に POST |
| **notify** | `src/notify.rs` | lettre SMTP メール通知 |
| **error** | `src/error.rs` | `ScraperError` (thiserror) |

## entrypoint (`src/main.rs`)

- Axum Router: `GET /health`、`POST /scrape`
- `/scrape` body: `start_date` / `end_date` (省略時前日) / `comp_id` (省略時全企業) / `skip_upload`
- レスポンスは **SSE** (`ProgressEvent` を `mpsc` で stream、step = login/download/upload/done)
- `AppState.comp_locks`: comp_id 別 `Arc<Mutex<()>>` を `HashMap` で管理し同一企業を直列化

## gotcha (CLAUDE.md 由来)

- **同一 comp_id への並列 `/scrape` は race condition** (修正済 commit `9253efd`): ① DL dir を
  PID+nanos でユニーク化、② comp_id 別 Mutex で直列化。別 comp_id は並列のまま。
- **本番ログを意図的に残してある**: `Actual date field values after typing` (西暦/和暦判定)、
  `ZIP contents for comp_id=...` (KUDGIVT 欠落調査用)。`KUDGIVT.csv not found` 系が出たら
  Cloud Run logs で `ZIP contents` を確認する。
- **daiun-salary は dtako-scraper の SSE プロキシ** — daiun-salary 単体に対策を入れても無意味。
- **CI 自動 deploy 無し** → main に merge/push しても本番に届かない。`./deploy.sh` を手動で叩く
  (deploy 前に user に AskUserQuestion で確認)。過去に deploy 漏れで 3 日間バグが残った前例あり。

## CI / deploy から見た立ち位置

- **手動 `./deploy.sh`**: `docker build` → GHCR (`ghcr.io/ohishi-exp/dtako-scraper:latest`) push →
  Cloud Run `dtako-scraper` (asia-northeast1) deploy。Cloud Run は AR remote-repo
  (`asia-northeast1-docker.pkg.dev/cloudsql-sv/daiun-salary/...`) 経由で pull。
- 本番起動 path = Cron `dtako-scraper-daily` (`0 1 * * *` Asia/Tokyo) の日次実行。
- Dockerfile は 3-stage: rust builder → chromedp/headless-shell → debian-slim runtime
  (`CHROME_PATH=/headless-shell/headless-shell`)。`--no-allow-unauthenticated`, 2Gi/2cpu。
- env: `DTAKO_ACCOUNTS` / `DAIUN_SALARY_URL` / `DOWNLOAD_DIR` / `SMTP_*` / `MAIL_TO` (deploy.sh が .env から注入)。

## 関連 skill

- `package-publish-debug` — GHCR push denied / AR remote-repo proxy / Cloud Run の ghcr image 拒否時
- `cross-repo-symbol-index` — この per-repo map の運用方針 (generated-from 鮮度 hook)
