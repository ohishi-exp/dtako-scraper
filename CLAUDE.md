# CLAUDE.md

## Project Overview

Dtakolog CSV スクレイパー。theearth-np.com から csvdata.zip を自動取得し、daiun-salary API にアップロードする。

## Tech Stack

- **Language:** Rust
- **Web Framework:** Axum 0.8
- **Browser Automation:** chromiumoxide (CDP)
- **HTTP Client:** reqwest (multipart upload)
- **Runtime:** Docker + chromedp/headless-shell

## Build & Run

```bash
cargo build              # ビルド
cargo run                # サーバー起動 (要 .env)
docker build -t dtako-scraper .  # Docker イメージビルド
./deploy.sh              # Cloud Run デプロイ
```

## API

- `GET /health` — ヘルスチェック
- `POST /scrape` — スクレイピング実行
  ```json
  {
    "start_date": "2026-03-01",
    "end_date": "2026-03-13",
    "comp_id": "27324455"  // 省略時は全企業
  }
  ```

## Config (環境変数)

- `DTAKO_ACCOUNTS` — 企業アカウント JSON 配列
- `DAIUN_SALARY_URL` — daiun-salary API URL
- `DOWNLOAD_DIR` — ダウンロード先
- `PORT` — サーバーポート (default: 8080)
- `CHROME_PATH` — Chrome/headless-shell パス

## Related Projects

- `/home/yhonda/rust/daiun-salary` — 給与管理バックエンド（アップロード先）
- `/home/yhonda/rust/browser-render_rust` — 参考実装（chromiumoxide パターン）

<!-- migrated from memory/feedback_*.md (2026-05-11) -->

## 運用上の罠

### 同一 comp_id への並列 `/scrape` は race condition

`/scrape` は **同一 comp_id への並列実行に弱い**。原因 2 つ:
1. ダウンロードディレクトリが `/app/downloads/{comp_id}/` のみで分割 → 並列 call が `remove_dir_all` で互いの ZIP を消す
2. theearth-np.com は同一 comp_id+user で複数セッションを許さない → 片方の login が壊れる

**修正済** (commit `9253efd`, 2026-04-27 deploy):
- account_dir に PID+nanos を足してユニーク化
- `AppState` に comp_id 別 `tokio::Mutex` を持たせて直列化
- 直列化のオーバーヘッドは ~0、別 comp_id は並列のまま

**ログ** (本番に残してある):
- `Actual date field values after typing` (西暦/和暦判定の検証用)
- `ZIP contents for comp_id=...` (KUDGIVT 欠落調査用)

今後 KUDGIVT.csv not found 系の症状が出たら Cloud Run logs で `ZIP contents` を確認。
daiun-salary は dtako-scraper の SSE プロキシなので、daiun-salary 単体に対策入れても意味なし。

事例: 2026-04-27 手動 `/scrape` 検証中に「KUDGIVT.csv not found in ZIP」エラー
(`feedback_dtako_scraper_concurrency`)。

### デプロイは **手動 ./deploy.sh** (CI 自動 deploy 無し)

このリポジトリは **GitHub Actions の自動デプロイが無い**。`./deploy.sh` (GHCR push +
Cloud Run deploy) を手動で叩かないと本番が更新されない。

- main に merge / push しても本番に届かない → user に "deploy しますか？" を AskUserQuestion で確認してから `./deploy.sh`
- `deploy.sh` は GHCR (`ghcr.io/ohishi-exp/dtako-scraper:latest`) に push →
  Cloud Run が AR remote-repo (`asia-northeast1-docker.pkg.dev/cloudsql-sv/daiun-salary/`) 経由で pull
- Cron `dtako-scraper-daily` (asia-northeast1, `0 1 * * *` Asia/Tokyo) が日次起動の本番 path
- 本来は `.github/workflows/deploy.yml` を追加すべき (TODO)

`feedback_no_direct_deploy` の「デプロイは PR 経由」ルールは CI auto-deploy 前提の repo 向け
であり、ここは例外。2026-04-24 PR #2 で JST 修正が merge されていたが deploy 漏れで
3 日間バグが残ったまま稼働していた前例あり (`feedback_dtako_scraper_manual_deploy`)。
