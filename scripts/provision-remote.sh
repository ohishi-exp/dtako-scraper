#!/bin/bash
# device credential (tenant_id 単位) を Kagoya VPS の /opt/dtako-scraper/.env に
# 追記し、既存 image で container を再起動する。build/push は行わない
# (device pairing 専用、code deploy は .github/workflows/deploy.yml が担当)。
#
# dtako-scraper は複数 tenant を扱うため、browser-render-rust の
# scripts/deploy-remote.sh (単一 DEVICE_ID/DEVICE_SECRET を upsert) とは異なり、
# `DTAKO_DEVICE_CREDENTIALS` という 1 個の JSON blob (tenant_id -> {device_id,
# device_secret}) に対して jq でエントリを 1 つ merge する。
#
# 必須 env:
#   DEPLOY_SSH_HOST  … 接続先 SSH ホスト (例: KAGOYA_VPS_HOST の @ 以降)
#   TENANT_ID        … pairing した tenant の UUID
#   DEVICE_ID        … pairing で発行された device_id
#   DEVICE_SECRET    … pairing で発行された device_secret
#
# 任意 env:
#   DEPLOY_SSH_USER   … SSH ユーザー (default: ubuntu)
#   AUTH_WORKER_URL   … 空でなければ .env の AUTH_WORKER_URL も upsert
#   CONTAINER_NAME    … デプロイ済み container 名 (default: dtako-scraper)
#   APP_DIR           … VPS 上のアプリディレクトリ (default: /opt/dtako-scraper)
#   DEPLOY_HEALTH_PORT … health check する remote localhost ポート (default: 8081)
#   IMAGE             … 再起動に使う image (default: ghcr.io/ohishi-exp/dtako-scraper:latest)
#
# 失敗 (ssh / health) は即 exit != 0 で loud fail する。
set -euo pipefail

SSH_USER="${DEPLOY_SSH_USER:-ubuntu}"
TARGET_HOST="${DEPLOY_SSH_HOST:?DEPLOY_SSH_HOST is required}"
TARGET="$SSH_USER@$TARGET_HOST"
TENANT_ID="${TENANT_ID:?TENANT_ID is required}"
DEVICE_ID="${DEVICE_ID:?DEVICE_ID is required}"
DEVICE_SECRET="${DEVICE_SECRET:?DEVICE_SECRET is required}"
AUTH_WORKER_URL="${AUTH_WORKER_URL:-}"
CONTAINER_NAME="${CONTAINER_NAME:-dtako-scraper}"
APP_DIR="${APP_DIR:-/opt/dtako-scraper}"
HEALTH_PORT="${DEPLOY_HEALTH_PORT:-8081}"
IMAGE="${IMAGE:-ghcr.io/ohishi-exp/dtako-scraper:latest}"

echo "=== Provisioning device credential on ${TARGET} (tenant=${TENANT_ID}) ==="

# secret は SSH env-var 前置き経路でのみ渡す (positional arg / heredoc 埋め込みにしない)。
if ! ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes "$TARGET" \
    TENANT_ID="$TENANT_ID" DEVICE_ID="$DEVICE_ID" DEVICE_SECRET="$DEVICE_SECRET" \
    AUTH_WORKER_URL="$AUTH_WORKER_URL" \
    bash -s -- "$IMAGE" "$CONTAINER_NAME" "$APP_DIR" "$HEALTH_PORT" <<'REMOTE_SCRIPT'
set -e
IMAGE="$1"
CONTAINER_NAME="$2"
APP_DIR="$3"
HEALTH_PORT="$4"
ENV_FILE="$APP_DIR/.env"

touch "$ENV_FILE"
chmod 600 "$ENV_FILE"

# AUTH_WORKER_URL は単純な line upsert。
if [ -n "${AUTH_WORKER_URL:-}" ]; then
    tmp="${ENV_FILE}.tmp.$$"
    grep -v '^AUTH_WORKER_URL=' "$ENV_FILE" > "$tmp" 2>/dev/null || true
    printf 'AUTH_WORKER_URL=%s\n' "$AUTH_WORKER_URL" >> "$tmp"
    mv "$tmp" "$ENV_FILE"
    echo "  .env updated: AUTH_WORKER_URL"
fi

# DTAKO_DEVICE_CREDENTIALS は tenant_id をキーにした JSON blob。jq で 1 エントリ merge する。
cur=$(grep -E '^DTAKO_DEVICE_CREDENTIALS=' "$ENV_FILE" | cut -d= -f2- || true)
[ -n "$cur" ] || cur='{}'
new=$(printf '%s' "$cur" | jq -c --arg t "$TENANT_ID" --arg id "$DEVICE_ID" --arg s "$DEVICE_SECRET" \
    '.[$t] = {device_id:$id, device_secret:$s}')
tmp="${ENV_FILE}.tmp.$$"
grep -v '^DTAKO_DEVICE_CREDENTIALS=' "$ENV_FILE" > "$tmp" 2>/dev/null || true
printf 'DTAKO_DEVICE_CREDENTIALS=%s\n' "$new" >> "$tmp"
chmod 600 "$tmp"
mv "$tmp" "$ENV_FILE"
echo "  .env updated: DTAKO_DEVICE_CREDENTIALS (tenant=${TENANT_ID})"

echo 'Restarting container with updated .env...'
docker stop "$CONTAINER_NAME" 2>/dev/null || true
docker rm "$CONTAINER_NAME" 2>/dev/null || true

mkdir -p "$APP_DIR/downloads"
docker run -d \
    --name "$CONTAINER_NAME" \
    --restart=unless-stopped \
    --init \
    -p 127.0.0.1:8081:8080 \
    -v "$APP_DIR/downloads:/app/downloads" \
    --env-file "$ENV_FILE" \
    --shm-size=1g \
    --ulimit nofile=65536:65536 \
    --security-opt seccomp=unconfined \
    "$IMAGE"

echo 'Waiting for health check...'
for i in $(seq 1 15); do
    if curl -sf "http://localhost:${HEALTH_PORT}/health" > /dev/null 2>&1; then
        echo 'Health check passed!'
        docker ps -f "name=${CONTAINER_NAME}"
        exit 0
    fi
    echo "Waiting... (${i}/15)"
    sleep 2
done

echo 'Health check failed!'
docker logs --tail=200 "$CONTAINER_NAME" || true
exit 1
REMOTE_SCRIPT
then
  echo "::error::provision failed on remote host ${TARGET_HOST}" >&2
  exit 1
fi

echo "=== Done! device credential provisioned for tenant=${TENANT_ID} on ${TARGET_HOST} ==="
