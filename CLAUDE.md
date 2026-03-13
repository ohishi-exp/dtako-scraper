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
