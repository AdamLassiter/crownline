# syntax=docker/dockerfile:1

FROM rust:1.95.0-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src
COPY assets ./assets
RUN cargo build --locked --release -p crownline_server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 crownline \
    && useradd --system --uid 10001 --gid crownline --home-dir /var/lib/crownline crownline \
    && install --directory --owner=crownline --group=crownline --mode=0700 /var/lib/crownline
COPY --from=builder /workspace/target/release/crownline_server /usr/local/bin/crownline-server

ENV CROWNLINE_BIND=0.0.0.0:5000 \
    CROWNLINE_DATABASE_PATH=/var/lib/crownline/crownline.sqlite3 \
    CROWNLINE_DATABASE_DURABILITY=full \
    CROWNLINE_LOG_FORMAT=json \
    CROWNLINE_SHUTDOWN_SECONDS=15
VOLUME ["/var/lib/crownline"]
EXPOSE 5000
USER crownline:crownline
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:5000/health/ready"]
ENTRYPOINT ["/usr/local/bin/crownline-server"]
