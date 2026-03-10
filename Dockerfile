# ---------- Stage 1: Build frontend ----------
FROM node:22-slim AS frontend-build
RUN corepack enable && corepack prepare pnpm@latest --activate
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm exec svelte-kit sync && pnpm build

# ---------- Stage 2: Build backend ----------
FROM rust:1.94-bookworm AS backend-build
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
# Cache dependency build
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY backend/src ./src
RUN touch src/main.rs && cargo build --release

# ---------- Stage 3: Runtime ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    gosu \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user (overridden by entrypoint with PUID/PGID).
RUN groupadd -g 1000 sharky && useradd -u 1000 -g sharky -m sharky

COPY --from=backend-build /app/backend/target/release/sharky-fish /usr/local/bin/sharky-fish
COPY --from=frontend-build /app/frontend/build /srv/frontend
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 3000

ENTRYPOINT ["/entrypoint.sh"]
