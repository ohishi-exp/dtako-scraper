# CLAUDE.md

## Project Overview

Dtakolog CSV スクレイパー。theearth-np.com から csvdata.zip を自動取得し、
rust-alc-api (`POST /api/upload`) にアップロードする (daiun-salary への送信ではない、下記
「運用上の罠」参照)。

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

- `DTAKO_ACCOUNTS` — 企業アカウント JSON 配列 (`comp_id`/`user_name`/`user_pass`/`tenant_id`)。
  企業ごとに `tenant_id` が異なりうる (マルチテナント)
- `AUTH_WORKER_URL` — auth-worker URL (device token 発行 + `/device-data-proxy` 経由で
  rust-alc-api に到達する。default `https://auth.ippoan.org`)
- `DTAKO_DEVICE_CREDENTIALS` — `tenant_id -> {device_id, device_secret}` の JSON。
  rust-alc-api への upload に使う device credential。`.github/workflows/provision-device.yml`
  が tenant ごとに発行して VPS の `.env` に自動投入する (下記「運用上の罠」参照)
- `DOWNLOAD_DIR` — ダウンロード先
- `PORT` — サーバーポート (default: 8080)
- `CHROME_PATH` — Chrome/headless-shell パス

旧 `DAIUN_SALARY_URL` (直接 rust-alc-api の Cloud Run URL を叩く方式) は device credential
方式への移行 (2026-07-01) に伴い廃止。

## Related Projects

- `/home/yhonda/rust/rust-alc-api` — **実際のアップロード先**。
  `crates/alc-dtako/src/dtako_upload.rs::upload_zip` (`POST /api/upload`) が受け口。
  本番 Cloud Run は #434 lockdown 済みのため、直接 HTTP POST ではなく auth-worker
  `/device-data-proxy` 経由でしか到達できない
- `/home/yhonda/rust/auth-worker` — device credential の発行元 (`/device/pair-internal`,
  `/device/token`, `/device-data-proxy`)
- `/home/yhonda/rust/daiun-salary` — 別プロジェクト (北海大運の給与管理システム)。**アップロード先ではない**
  (2026-07-01 に一度誤認して `/internal/upload` に変更する事故があった、下記参照)
- `/home/yhonda/rust/browser-render_rust` — dtakolog 送信で同じ device credential 方式
  (`device-dtako-ingest` role 共用) を先行実装した参考実装。`src/device_auth.rs` /
  `.github/workflows/provision-device.yml` が移植元

<!-- migrated from memory/feedback_*.md (2026-05-11) -->

## 運用上の罠

### アップロード先は rust-alc-api の `/api/upload`、device credential 経由でしか到達できない (Refs #14, rust-alc-api#434)

2026-07-01 に本番で `POST /api/upload` が 403 Forbidden で失敗する事故があり、調査過程で
以下が判明・確定した:

1. **`DAIUN_SALARY_URL` という旧 env var 名から daiun-salary (別リポジトリ) への送信だと
   誤解しやすいが、実体は rust-alc-api の Cloud Run URL** だった。一時的に daiun-salary の
   実装だと誤認して `/internal/upload` (daiun-salary 側のパスで rust-alc-api には存在しない)
   に変更する誤修正が merge されたが、`/api/upload` (`crates/alc-dtako/src/dtako_upload.rs::
   upload_zip`、`require_tenant_header` 配下) に訂正済み
2. しかし `/api/upload` 自体が正しくても、rust-alc-api 本番 Cloud Run は **#434 (Cloud Run
   IAM lockdown)** で `allUsers` invoker 権限が撤去済みのため、直接 HTTP POST は
   **Google Front End (GFE) レベルで 403 Forbidden になる** (`<title>403 Forbidden</title>`
   `Your client does not have permission to get URL ... from this server.` という定型文言)
3. 恒久対応として、browser-render-rust が先行実装していた **device credential +
   auth-worker `/device-data-proxy` 経由**の方式に移行した (`src/device_auth.rs` /
   `src/scraper/upload.rs`)。`{AUTH_WORKER_URL}/device-data-proxy/api/upload` を
   device JWT (`Authorization: Bearer`) 付きで叩く。X-Tenant-ID は device JWT の
   `tenant_id` claim から proxy が注入するため、client からは送らない (送っても無視される)

#### マルチテナント対応 (browser-render-rust との違い)

browser-render-rust は 1 device credential = 1 tenant 固定だが、dtako-scraper は
`DTAKO_ACCOUNTS` の企業ごとに `tenant_id` が異なりうる。**device-data-proxy は JWT に
焼き込まれた tenant_id claim を無条件に信頼する**ため (なりすまし防止の要)、1 credential
では複数 tenant を跨げない → **`DTAKO_DEVICE_CREDENTIALS` で tenant_id ごとに credential
を保持し、account.tenant_id でルックアップして使う**。

**wrong-tenant silent write に注意**: VPS の `.env` に tenant A の credential を tenant B の
キーで誤登録すると、200 成功のまま別テナントに書き込まれてしまう (proxy 側は forbidden を
返さない)。`src/device_auth.rs::mint_device_token_for_tenant` が `/device/token` 応答の
`tenant_id` と呼び出し元が期待する `tenant_id` を assert し、不一致なら loud fail する
(唯一のクライアント側防御)。

#### role は browser-render-rust と共用 (`device-dtako-ingest`)

新規 role を切らず、既存 `device-dtako-ingest` role の allowlist に `/api/upload` を追加する
方針にした (ippoan/auth-worker#341)。理由: browser-render-rust (dtakolog bulk ingest) と
dtako-scraper (ZIP upload) は同一 Kagoya VPS・同一運用チーム・同一機能ドメイン (dtako データの
rust-alc-api への ingest) であり、role を分けるとメンテナンスコストが増えるだけで
セキュリティ上のメリットが薄い。device credential (device_id/device_secret) 自体は
サービス・テナントごとに個別発行するため、rotate/revoke の粒度は role 統一後も維持される。

#### device credential の provision

`.github/workflows/provision-device.yml` (手動 `workflow_dispatch`) が tenant_id ごとに
`/device/pair-internal` を叩いて device credential を発行し、`scripts/provision-remote.sh`
経由で VPS の `.env` の `DTAKO_DEVICE_CREDENTIALS` に merge + container 再起動する。

- Actions → **Provision device credential** → Run workflow で `tenant_id` を入力して実行
- **企業 (tenant) を追加するたびに、その tenant_id で 1 回実行する必要がある**
  (browser-render-rust の 1-tenant-固定運用とは異なる)
- **`tenant_id` 欄には UUID (rust-alc-api の `tenants.id`) を入れる。`comp_id` (数字、
  例: `27324455`) を間違えて入れないこと** — 2026-07-01 に一度誤入力しかけた事例あり
- `INTERNAL_SHARED_SECRET` は ohishi-exp org secret (browser-render-rust と共用、CI にだけ
  置く。VPS には配らない)

**初回実行時の既知バグ (2026-07-01、修正済み)**: `scripts/provision-remote.sh` に GHCR
ログインが無く、`docker run` が `Unable to find image ... denied` で失敗した (`.env` の
`DTAKO_DEVICE_CREDENTIALS` 自体は正常に書き込まれるが container 再起動だけ失敗する)。
deploy.yml と同じ GHCR login パターン (CI の job 限り `GITHUB_TOKEN` → 無ければ VPS の
`.env` の `GHCR_TOKEN` に fallback) を追加して修正した。同じ症状が再発したら
`docker login ghcr.io` 周りを疑う。

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

#### concurrency は 2 階層 (2026-07-01 修正、browser-render-rust に揃えた)

- **workflow 全体**: `group: dtako-scraper-ci-<PR番号 or ref>` + `cancel-in-progress: true`。
  同一 PR への新規 push で古い CI run (test/build) をキャンセルする通常の CI hygiene 用。
  他 PR の run には影響しない。
- **`deploy` job だけ**: `group: deploy-vps` + `cancel-in-progress: false`
  (job-level concurrency)。VPS に実際に触れる操作 (docker stop/rm/run) はこの group で
  直列化するが、先行 run を kill せず完走を待つ。`provision-device.yml` も同じ
  `deploy-vps` group + `cancel-in-progress: false` を使うので、deploy と provision が
  同時に VPS の container を取り合っても片方が完走してからもう片方が始まる。

**以前は workflow 全体を `deploy-vps` group + `cancel-in-progress:true` にしていたが、
これだと他 PR の deploy job が SSH 中 (docker stop/rm 済み・run 前) でも新しい PR push で
キャンセルされ、VPS の container が停止したまま再起動されない事故が起こり得た**
(browser-render-rust の `ci.yml` `deploy` job は元から job-level `cancel-in-progress:false`
でこれを避けている設計だった)。

#### `push: branches: [main]` トリガー (cache 書き戻し専用、2026-07-01 追加)

`test` job の Swatinem/rust-cache は `save-if: github.event_name == 'workflow_dispatch' ||
github.ref == 'refs/heads/main'` だが、**deploy.yml が元々 `pull_request` トリガーしか
持たなかったため、この条件が (手動 workflow_dispatch を除いて) 一生 true にならず
sccache/rust-cache が実質更新されないバグがあった** (browser-render-rust の `ci.yml` は
`push: [master]` を持つのでこの問題が起きない)。

`push: branches: [main]` を追加してこれを解消したが、**`deploy` job は
`if: github.event_name != 'push'` で push イベント時は skip する** (main merge 時点の
commit は PR の時点で既に VPS に preview deploy 済みのため、push 時の再 deploy は冗長)。
push イベントで走るのは `test` (cache 書き戻し) と `build` (docker `:latest` タグの GHA
layer cache 書き戻し) のみ。`disable-auto-merge`/`auto-merge` job は元々
`github.event_name == 'pull_request'` 限定なので push イベントでは走らない (影響なし)。

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
| `INTERNAL_SHARED_SECRET` | org (browser-render-rust と共用) | `provision-device.yml` が auth-worker `/device/pair-internal` を叩くため |

#### VPS 側の前提 (一度だけ準備)

- `/opt/dtako-scraper/.env` (既存 `.env` 相当 + `GHCR_TOKEN=<read:packages PAT>`)
- `/opt/dtako-scraper/downloads/`
- docker インストール済み、CI runner の SSH 公開鍵を `authorized_keys` に登録
- cron `dtako-scraper-daily` (`0 1 * * *` Asia/Tokyo) が VPS 上で container を叩く形 →
  **tag deploy は cron 時刻 (深夜 01:00 JST) を避けて push** する (container 再起動で
  scrape が中断するため)

参考: 同じ作者の `yhonda-ohishi-pub-dev/browser-render-rust` `scripts/deploy-kagoya.sh`
が deploy.yml の deploy job ロジックの参照元。
