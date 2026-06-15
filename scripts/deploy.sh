#!/bin/bash
# 手動 deploy (CI が動かせない時の fallback)。
#
# 通常は PR を main に merge → dev-release.yml が dev-{N} tag を採番 →
# deploy.yml が VPS deploy、で全自動 (CLAUDE.md「Deploy」参照)。
#
# 本 script は CI と同じロジックを手元から叩ける形にしただけ。
# browser-render-rust の scripts/deploy-kagoya.sh をモデルにしている。
#
# 必須環境変数 (.env または env):
#   KAGOYA_VPS_HOST  — "user@host" 形式 (例: "ubuntu@133.18.162.83")
#
# 任意:
#   $1           — image tag (省略時は git rev-parse --short HEAD)
#
# 前提:
#   - 手元に docker + gh + GHCR push 権限 (gh auth login 済み)
#   - VPS の /opt/dtako-scraper/.env に GHCR_TOKEN + DTAKO_ACCOUNTS 等が配置済み
#   - 手元 SSH 鍵で VPS にログイン可能

set -euo pipefail

# .env から KAGOYA_VPS_HOST を読みたい時用
if [ -f .env ]; then
    set -a
    # shellcheck disable=SC1091
    source .env
    set +a
fi

IMAGE="ghcr.io/ohishi-exp/dtako-scraper"
TAG="${1:-$(git rev-parse --short HEAD)}"
CONTAINER_NAME="dtako-scraper"
APP_DIR="/opt/dtako-scraper"

: "${KAGOYA_VPS_HOST:?KAGOYA_VPS_HOST must be set (e.g. 'ubuntu@133.18.162.83')}"

echo "=== dtako-scraper manual deploy ==="
echo "Image:  ${IMAGE}:${TAG}"
echo "Target: ${KAGOYA_VPS_HOST}"
echo

echo "=== Building Docker image ==="
docker build \
    --build-arg CARGO_BUILD_JOBS=2 \
    -t "${IMAGE}:${TAG}" \
    -t "${IMAGE}:latest" \
    .

echo
echo "=== Pushing to GHCR ==="
docker push "${IMAGE}:${TAG}"
docker push "${IMAGE}:latest"

echo
echo "=== Deploying via SSH ==="
ssh "$KAGOYA_VPS_HOST" \
    IMAGE="${IMAGE}:${TAG}" \
    CONTAINER_NAME="$CONTAINER_NAME" \
    APP_DIR="$APP_DIR" \
    'bash -s' <<'REMOTE'
set -e

: "${IMAGE:?IMAGE required}"
: "${CONTAINER_NAME:?CONTAINER_NAME required}"
: "${APP_DIR:?APP_DIR required}"

if [ -f "$APP_DIR/.env" ]; then
    GHCR_TOKEN=$(grep -E '^GHCR_TOKEN=' "$APP_DIR/.env" | cut -d= -f2-)
    if [ -n "$GHCR_TOKEN" ]; then
        echo "$GHCR_TOKEN" | docker login ghcr.io -u ohishi-exp --password-stdin >/dev/null
    fi
fi

echo "Pulling $IMAGE ..."
docker pull "$IMAGE"

echo "Stopping existing container..."
docker stop "$CONTAINER_NAME" 2>/dev/null || true
docker rm "$CONTAINER_NAME" 2>/dev/null || true

echo "Starting new container..."
mkdir -p "$APP_DIR/downloads"
docker run -d \
    --name "$CONTAINER_NAME" \
    --restart=unless-stopped \
    --init \
    -p 127.0.0.1:8080:8080 \
    -v "$APP_DIR/downloads:/app/downloads" \
    --env-file "$APP_DIR/.env" \
    --shm-size=1g \
    --ulimit nofile=65536:65536 \
    --security-opt seccomp=unconfined \
    "$IMAGE"

echo "Waiting for health check..."
for i in $(seq 1 15); do
    if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
        echo "Health check passed!"
        docker ps -f name="$CONTAINER_NAME"
        docker image prune -af --filter 'until=24h' || true
        exit 0
    fi
    echo "Waiting... ($i/15)"
    sleep 2
done

echo "Health check failed!"
docker logs --tail=200 "$CONTAINER_NAME" || true
exit 1
REMOTE

echo
echo "=== Deploy complete ==="
echo "Image: ${IMAGE}:${TAG}"
