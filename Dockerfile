# Stage 1: Build Rust binary
FROM rust:1.88-slim-bookworm AS builder

ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Dependency caching
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release -j ${CARGO_BUILD_JOBS} && rm -rf src

# Build actual source
COPY src ./src
RUN touch src/main.rs && cargo build --release -j ${CARGO_BUILD_JOBS}

# Stage 2: Headless Chrome
FROM chromedp/headless-shell:stable AS headless

# Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 ca-certificates fonts-liberation libasound2 libatk-bridge2.0-0 \
    libatk1.0-0 libcups2 libdbus-1-3 libdrm2 libgbm1 libgtk-3-0 \
    libnspr4 libnss3 libxcomposite1 libxdamage1 libxfixes3 libxkbcommon0 \
    libxrandr2 dumb-init \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r appuser && useradd -r -g appuser appuser

WORKDIR /app

COPY --from=builder /build/target/release/dtako-scraper .
COPY --from=headless /headless-shell /headless-shell

ENV CHROME_PATH=/headless-shell/headless-shell
ENV CHROMIUM_PATH=/headless-shell/headless-shell

RUN mkdir -p /app/downloads && chown -R appuser:appuser /app
USER appuser
EXPOSE 8080

ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["./dtako-scraper"]
