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

# Deploy: 通常は不要 (PR を main に merge すると CI が自動 deploy)。
# 緊急時の手動 deploy fallback (要: 手元 docker + VPS への SSH 鍵):
KAGOYA_VPS_HOST="ubuntu@<vps-ip>" ./scripts/deploy.sh
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

今後 KUDGIVT.csv not found 系の症状が出たら **VPS の `docker logs dtako-scraper`** で
`ZIP contents` を確認。daiun-salary は dtako-scraper の SSE プロキシなので、
daiun-salary 単体に対策入れても意味なし。

事例: 2026-04-27 手動 `/scrape` 検証中に「KUDGIVT.csv not found in ZIP」エラー
(`feedback_dtako_scraper_concurrency`)。

### Deploy: PR トリガーで CI 自動化

deploy 先は **自前 VPS (docker / SSH)** — `browser-render-rust` と同じ Kagoya VPS の
docker container として動作する。Cloud Run ではない (旧 `deploy.sh` の
`gcloud run deploy` は実態と乖離していたため削除済み、2026-06-15)。

#### deploy.yml の serial chain (1 workflow にまとめて並列無駄を排除)

```
PR を main に向ける (= deploy.yml 起動)
  ├ disable-auto-merge (CI 開始時に PR の auto-merge を一旦 disable)
  └ test (cargo fmt + cargo test)
      └ build (docker build + GHCR push to :pr-{N}-{sha} + :latest)
          └ deploy (SSH で VPS に docker pull + container 入れ替え + health check 30s)
              └ auto-merge (ci-workflows/auto-merge.yml@main、CI_APP_ID 明示渡し)
```

- **PR を出した時点でその PR のコミットが VPS に反映される** (preview deploy 兼ステージング)
- reviewer は実動作を見て merge 可否を判断
- auto-merge が enable されれば test/build/deploy 全 green で自動 merge
- concurrency: `deploy-vps` で同時走行を直列化 (新 PR push は走行中の古い run を cancel)
- 緊急 deploy: `gh workflow run deploy.yml -f ref=<sha-or-branch>` (workflow_dispatch)
  または手元から `KAGOYA_VPS_HOST=ubuntu@... ./scripts/deploy.sh`

#### 機密の扱い

**`DTAKO_ACCOUNTS` / `SMTP_*` / `GHCR_TOKEN` は VPS の `/opt/dtako-scraper/.env` に
置いたまま `--env-file` で渡る = GitHub Actions / workflow YAML には一切載らない**。
鍵 rotate は VPS の `.env` を直接編集 + container 再起動。

#### 必要な GitHub org / repo secrets

`KAGOYA_VPS_*` は同 Kagoya VPS に deploy する他 repo (browser-render-rust 等)
とも共有する想定で、ohishi-exp **org level secret** に格納し `secrets: inherit`
で読む (= GCP Secret Manager の SoT もこの名前で 1 つ)。

| 名前 | scope | 値 |
|---|---|---|
| `KAGOYA_VPS_SSH_KEY` | org | Kagoya VPS 用 SSH 秘密鍵 |
| `KAGOYA_VPS_HOST` | org | `ubuntu@<IP>` (browser-render と同 VPS) |
| `CI_APP_ID` / `CI_APP_PRIVATE_KEY` | org (既存) | auto-merge job (ci-workflows/auto-merge.yml) が App token で merge するため。cross-org caller なので deploy.yml で `secrets:` 明示渡し |

#### VPS 側の前提 (一度だけ準備)

- `/opt/dtako-scraper/.env` (既存 `.env` 相当 + `GHCR_TOKEN=<read:packages PAT>`)
- `/opt/dtako-scraper/downloads/`
- docker インストール済み、CI runner の SSH 公開鍵を `authorized_keys` に登録
- cron `dtako-scraper-daily` (`0 1 * * *` Asia/Tokyo) が VPS 上で container を叩く形 →
  **tag deploy は cron 時刻 (深夜 01:00 JST) を避けて push** する (container 再起動で
  scrape が中断するため)

参考: 同じ作者の `yhonda-ohishi-pub-dev/browser-render-rust` `scripts/deploy-kagoya.sh`
が deploy.yml の deploy job ロジックの参照元。
