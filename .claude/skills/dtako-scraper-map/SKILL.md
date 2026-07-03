---
name: dtako-scraper-map
generated-from: dtako-scraper:a1d72e2
paths: [src/, docs/net780-binary-format.md, docs/vdf-format.md]
description: ohishi-exp/dtako-scraper (Rust + Axum + chromiumoxide ヘッドレス Chrome の Dtakolog CSV スクレイパー) の構造ナビゲーション。theearth-np.com から csvdata.zip を取得 → daiun-salary API に multipart upload する **Kagoya VPS docker service** (browser-render-rust と同 host を共有) の module 配置・SSE 進捗・PR トリガー CI deploy・運用 gotcha を 1 枚にまとめる。NET780 生データパーサー (`crates/net780`) は `ohishi-exp/net780-wasm` (`core/`+`wasm/` workspace) に完全移設済み (2026-07-03、Refs #18/#26) — 本 repo には残っていない。トリガー:「dtako-scraper」「Dtakolog」「csvdata.zip」「theearth-np」「chromiumoxide」「headless-shell」「KUDGIVT」「comp_id 並列」「daiun-salary upload」「Kagoya VPS」「VPS deploy」「F-VOS3020」「scrape-vehicle-setting」等。
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

## Cargo workspace 構成 (net780 移設済み、2026-07-03)

- root (`Cargo.toml` の `[package]`) = 通常の単一パッケージ scraper service バイナリ。
  workspace ではない (以前は `crates/net780` を workspace member として同居させて
  いたが完全撤去済み)。
- **NET780 生データパーサー (旧 `crates/net780`) は `ohishi-exp/net780-wasm` の
  `core/` に完全移設済み** (Refs #18、dtako-scraper#26 の `.gpd` marker-scan 修正
  込み)。理由: net780 crate は dtako-scraper 本体 (scraper バイナリ) から一切
  参照されておらず、`ohishi-exp/net780-wasm` の git dependency 用にワークスペース
  メンバーとして残していただけだった。cross-repo (private repo 間) 依存の運用
  コスト (`git`+`path` 併記不可の罠、修正のたびに 2 repo にまたがる PR が要る等)
  を避けるため、`ohishi-exp/dtako_vid_wasm` と同じ「`core/`+`wasm/` を 1 repo の
  workspace に同居」構成に統合した。
  **`crates/net780/` を本 repo に再追加しないこと** — NET780 パーサーへの変更は
  `ohishi-exp/net780-wasm` の `core/` で行う。`docs/net780-binary-format.md` /
  `docs/vdf-format.md` (フォーマット仕様書) は参照用としてこの repo に残っている。

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
  VPS の `docker logs dtako-scraper` で `ZIP contents` を確認する。
- **daiun-salary は dtako-scraper の SSE プロキシ** — daiun-salary 単体に対策を入れても無意味。
- **PR トリガー CI deploy** (`.github/workflows/deploy.yml` 1 本に統合済、2026-06-15)。
  PR を main に向けるとその commit が VPS に preview deploy される (= reviewer は実動作を見て
  merge 可否を判断)。tag や dev-release.yml のような中継経路はもう無い。
- **過去の罠** (今は解消): 旧 `deploy.sh` は Cloud Run を叩いていたが実態は VPS 運用で、
  CI 自動化前は deploy 漏れで 3 日間バグが残った前例あり (CI deploy で再発防止)。

## CI / deploy から見た立ち位置

- **`.github/workflows/deploy.yml`** — `on: pull_request branches: [main]` + `workflow_dispatch`。
  serial chain: `disable-auto-merge → test → build → deploy → auto-merge`。
  `concurrency: deploy-vps` で同時走行を直列化 (新 push が古い run を cancel)。
- **build job**: docker build (`CARGO_BUILD_JOBS=2`, GHA layer cache) → GHCR push
  (`ghcr.io/ohishi-exp/dtako-scraper:pr-{N}-{sha}` + `:latest`)。
  `permissions: packages: write` 必須 (workflow + job 両方に declare)。
- **deploy job**: `webfactory/ssh-agent@v0.9.0` で `KAGOYA_VPS_SSH_KEY` を ssh-agent に load
  → `ssh ubuntu@<vps>` で docker login (CI の `GITHUB_TOKEN` を渡す = VPS の `.env` に
  GHCR_TOKEN を置く必要なし) → `docker pull → stop/rm → docker run -d --env-file ... →
  health check 15 リトライ x 2s → docker image prune`。
- **auto-merge**: cross-org caller として `CI_APP_ID` / `CI_APP_PRIVATE_KEY` / `TAG_RELEASE_PAT`
  を `secrets:` 明示渡し (ohishi-exp → ippoan/ci-workflows は `secrets: inherit` 不可)。
- **本番起動 path** = Cron `dtako-scraper-daily` (`0 1 * * *` Asia/Tokyo) が VPS 上で
  `POST /scrape` を叩く形 → **PR の deploy は cron 時刻を避けて push** する
  (container 再起動で scrape が中断するため)。
- **Dockerfile** は 3-stage: rust builder → chromedp/headless-shell → debian-slim runtime
  (`CHROME_PATH=/headless-shell/headless-shell`、`--security-opt seccomp=unconfined`、
  `--shm-size=1g`、`--init`)。
- **機密の扱い**: `DTAKO_ACCOUNTS` / `DAIUN_SALARY_URL` / `SMTP_*` / `MAIL_TO` は VPS の
  `/opt/dtako-scraper/.env` に置いたまま `--env-file` で渡る = workflow YAML には一切載らない。
  GHCR pull token は CI の `GITHUB_TOKEN` を SSH 経由で渡すので VPS の `.env` には不要。

## 必要な secrets / vars

`KAGOYA_VPS_*` は browser-render-rust 等と同 VPS を共有する想定で **ohishi-exp org level secret**
として配布 (GCP Secret Manager が SoT、`secrets-inventory-gcp` proxy の App mode 経由で同期):

| 名前 | scope | 値 |
|---|---|---|
| `KAGOYA_VPS_SSH_KEY` | ohishi-exp org | Kagoya VPS 用 SSH 秘密鍵 |
| `KAGOYA_VPS_HOST` | ohishi-exp org | `ubuntu@<IP>` |
| `CI_APP_ID` / `CI_APP_PRIVATE_KEY` / `TAG_RELEASE_PAT` | ohishi-exp org | auto-merge job 用 |

`KAGOYA_VPS_*` を ohishi-exp に投入するには secrets-inventory-gcp の Cloud Run env で
**App mode** を有効化 (`GH_APP_ID_SECRET_NAME` + `GH_APP_PRIVATE_KEY_SECRET_NAME` セット)
する必要あり (Refs ippoan/secrets-inventory-gcp#51、2026-06-15)。

## 関連 skill

- `package-publish-debug` — GHCR push denied (package access 設定漏れ等)
- `secret-inject` / `secrets-inventory-gcp-map` — VPS secrets (`KAGOYA_VPS_*`) の投入経路
- `cross-repo-symbol-index` — この per-repo map の運用方針 (generated-from 鮮度 hook)
