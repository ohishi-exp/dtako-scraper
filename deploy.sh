#!/bin/bash
set -euo pipefail

# .env 読み込み
set -a
source .env
set +a

IMAGE=asia-northeast1-docker.pkg.dev/cloudsql-sv/daiun-salary/dtako-scraper:latest

echo "==> Building Docker image..."
docker build --build-arg CARGO_BUILD_JOBS=2 -t "$IMAGE" .

echo "==> Pushing to Artifact Registry..."
docker push "$IMAGE"

echo "==> Deploying to Cloud Run..."
gcloud run deploy dtako-scraper \
  --image "$IMAGE" \
  --region asia-northeast1 \
  --platform managed \
  --no-allow-unauthenticated \
  --port 8080 \
  --memory 2Gi \
  --cpu 2 \
  --timeout 600 \
  --min-instances 0 \
  --max-instances 1 \
  --set-env-vars "DTAKO_ACCOUNTS=${DTAKO_ACCOUNTS}" \
  --set-env-vars "DAIUN_SALARY_URL=${DAIUN_SALARY_URL}" \
  --set-env-vars "DOWNLOAD_DIR=/app/downloads"

echo "==> Done!"
gcloud run services describe dtako-scraper --region=asia-northeast1 --format="value(status.url)"
