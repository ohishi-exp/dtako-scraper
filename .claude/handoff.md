# 引き継ぎ (2026-06-15)

このセッションでは `ippoan/email-receiver#1` epic の sub-issue を一通り完了し、
さらに dtako-scraper の CI deploy 基盤を整備した。

## 完了 (このセッション中に merge 済み)

- ✅ ippoan/email-receiver#2 — Worker scaffold + dtako handler (subject parser + dispatcher)
- ✅ ippoan/rust-alc-api#415 — `alc-dtako` に dtako_tickets table + REST API (Refs #414)
- ✅ ohishi-exp/dtako-scraper#6 (close) → #9 — `POST /scrape-vehicle-setting` 実装 + CI deploy
  化 (test → build → deploy → auto-merge の serial chain、PR を main に向ければ Kagoya VPS
  に preview deploy)
- ✅ ippoan/secrets-inventory-gcp#55 — GH App mode 有効化 (`?gh_org=ohishi-exp` propagate 可)
- ✅ ippoan/claude-skills#71 — `secret-inject` skill に cross-org propagate の罠を追記

## 次にやること

1. **ohishi-exp/nuxt_dtako_logs#15** — `/tickets` 一覧 / `/tickets/{id}` 詳細 (印刷
   レイアウト + 設定内容 + close 用 QR) / `/tickets/close?token=...` close ページの実装。
   rust-alc-api の `GET /api/dtako/tickets` / `GET /api/dtako/tickets/{id}` /
   `POST /api/dtako/tickets/close` を叩く。これで epic #1 が end-to-end になる
2. **epic 全体の動作確認** — 実 SD カードエラー通知メールを受けて起票 → F-VOS3020 設定 ZIP
   DL → ticket に反映 → QR で close の e2e フロー
3. **VPS 側の確認** — dtako-scraper を port 8081 に移したので cron `dtako-scraper-daily` /
   daiun-salary プロキシ等の向き先 (`http://localhost:8080` → `8081`) を更新する必要がないか
   user に確認

## 注意点

- **dtako-scraper deploy は PR トリガー**: PR を main に向けるとその commit が自動で
  Kagoya VPS の docker container を入れ替える (= preview deploy)。merge 後の追加 deploy
  は不要。tag 中継 (dev-release.yml) や `./deploy.sh` の旧 Cloud Run 経路は廃止
- **VPS 上 port 配置**: browser-render-rust が `127.0.0.1:8080` 占有、dtako-scraper は
  `127.0.0.1:8081` (host) → 8080 (container) に変更済み
- **secret は GCP SoT + sync_from_gcp で propagate**: ohishi-exp に投入したい時は
  `inject-secret.sh --targets gcp` で GCP に置く → `mcp__secret-manger__sync_from_gcp`
  MCP tool で `gh_org=ohishi-exp` を指定 (App mode 有効化済、Refs ippoan/secrets-inventory-gcp#55)
- **VPS 側の前提** (1 度きり setup 済): `/opt/dtako-scraper/{downloads,.env}` 配置、
  .env は docker --env-file 形式 (シングルクォート不可)、Kagoya VPS の authorized_keys
  に CI 用公開鍵 (org secret `KAGOYA_VPS_SSH_KEY` の対) 登録済
- `Refs #N` を使う (`Closes/Fixes/Resolves #N` は禁止、auto-close 防止)

## 関連 PR / commit (永続リンク)

- ippoan/email-receiver#2 — scaffold
- ippoan/rust-alc-api#415 — alc-dtako dtako_tickets
- ohishi-exp/dtako-scraper#6, #7, #8, #9 — F-VOS3020 endpoint + CI deploy 整備
- ippoan/secrets-inventory-gcp#55 — App mode 有効化
- ippoan/claude-skills#71 — secret-inject skill 更新

## まだ open の関連 branch (CCoW 内に残っているもの)

- `dtako-scraper`: `claude/simplify-deploy-main-push` (PR #9 merged、削除可)、
  `claude/fix-deploy-yaml-parse` (PR #8 merged)、`claude/ci-deploy-vps` (PR #7 merged)
- `secrets-inventory-gcp`: `claude/enable-gh-app-mode` (PR #55 merged、削除可)
- `claude-skills`: `claude/secret-inject-cross-org-trap` (PR #71 merged、削除可)
