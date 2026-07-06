# CLAUDE.md

## Project Overview

Dtakolog CSV スクレイパー。theearth-np.com から csvdata.zip を自動取得し、
rust-alc-api (`POST /api/upload`) にアップロードする。**daiun-salary への送信ではない**
(env var 名 `DAIUN_SALARY_URL` に惑わされないこと。過去に誤認事故あり、詳細は skill 参照)。

## Tech Stack

Rust / Axum 0.8 / chromiumoxide (CDP) / reqwest (multipart) / Docker + chromedp/headless-shell

## Build & Run

```bash
cargo build              # ビルド
cargo run                 # サーバー起動 (要 .env)
docker build -t dtako-scraper .  # Docker イメージビルド

# Deploy: 通常は不要 (PR を main に merge すると CI が自動 deploy)。
# 緊急時の手動 deploy fallback (要: 手元 docker + VPS への SSH 鍵):
KAGOYA_VPS_HOST="ubuntu@<vps-ip>" ./scripts/deploy.sh
```

## 必ず守ること

- **アップロード先は rust-alc-api の `/api/upload`、device credential 経由でしか到達できない**
  (本番 Cloud Run は #434 lockdown 済みのため、直接 HTTP POST は 403 Forbidden になる)
- `provision-device.yml` 実行時、`tenant_id` 欄には UUID (rust-alc-api の `tenants.id`) を
  入れる。`comp_id` (数字、例: `27324455`) を間違えて入れないこと
- tag/PR deploy は cron `dtako-scraper-daily` (`0 1 * * *` Asia/Tokyo) の時刻 (深夜 01:00 JST)
  を避けて push する (container 再起動で scrape が中断するため)

詳細 (アーキテクチャ・経緯・gotcha) は dtako-scraper-map skill を参照。
</content>
