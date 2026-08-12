FROM rust:bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends clang cmake pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
RUN cargo install dioxus-cli --version 0.7.10 --locked \
    && cargo install wasm-bindgen-cli --version 0.2.127 --locked

WORKDIR /workspace
COPY . .
RUN cargo xtask bundle

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/target/release/simple-table-web /usr/local/bin/simple-table-web

ENV IP=0.0.0.0
ENV PORT=8080
EXPOSE 8080

CMD ["simple-table-web"]
