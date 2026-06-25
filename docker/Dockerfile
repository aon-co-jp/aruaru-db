# aruaru-DB Dockerfile
# マルチアーキ: linux/amd64, linux/arm64
# docker build --platform linux/amd64,linux/arm64 -t aruaru-db .

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# アーキに合わせてバイナリを COPY
ARG TARGETARCH
COPY dist/aruaru-server-linux-${TARGETARCH}/aruaru-server /usr/local/bin/aruaru-server
RUN chmod +x /usr/local/bin/aruaru-server

# データディレクトリ
VOLUME ["/data"]

# PostgreSQL 互換ポート + GraphQL ポート
EXPOSE 5432 4000

ENTRYPOINT ["aruaru-server"]
CMD ["--data", "/data", "--log-level", "info"]

# ── ヘルスチェック ──────────────────────────────────────────
HEALTHCHECK --interval=10s --timeout=5s --retries=3 \
    CMD pg_isready -h localhost -p 5432 -U root || exit 1
