FROM rust:bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends clang cmake pkg-config libssl-dev musl-tools \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown x86_64-unknown-linux-musl
RUN cargo install dioxus-cli --version 0.7.10 --locked \
    && cargo install wasm-bindgen-cli --version 0.2.127 --locked

WORKDIR /workspace
COPY . .
RUN cargo xtask bundle --target x86_64-unknown-linux-musl

FROM alpine:3.20 AS runtime

RUN apk add --no-cache ca-certificates

COPY --from=builder /workspace/target/release/simple-table-web /usr/local/bin/simple-table-web

ENV IP=0.0.0.0
ENV PORT=8080
EXPOSE 8080

CMD ["simple-table-web"]
