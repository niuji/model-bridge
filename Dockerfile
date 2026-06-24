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

# 运行目录设在 /data（VOLUME）：相对路径的 model-bridge.db 与 providers.json
# 会落在此处，使数据卷真正持久化。providers.json 仍需挂载或自行放入 /data。
WORKDIR /data

ENV RUST_LOG=info
EXPOSE 10010 10020
VOLUME ["/data"]
CMD ["model-bridge", "--config", "/etc/model-bridge.toml"]
