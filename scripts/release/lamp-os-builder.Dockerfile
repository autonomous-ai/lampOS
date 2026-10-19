ARG RUST_VERSION=1.96.0
FROM rust:${RUST_VERSION}-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config clang libclang-dev libasound2-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
