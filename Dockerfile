FROM rust:1.97-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/rojakface-server /usr/local/bin/rojakface-server
COPY config/ ./config/
COPY templates/ ./templates/

EXPOSE 8005

CMD ["rojakface-server"]
