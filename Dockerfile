FROM rust:1.86-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY web ./web
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/model-bridge /usr/local/bin/model-bridge
COPY model-bridge.toml /etc/model-bridge.toml

ENV RUST_LOG=info
EXPOSE 8080
VOLUME ["/data"]
CMD ["model-bridge", "--config", "/etc/model-bridge.toml"]
