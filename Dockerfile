FROM rust:1.95-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/tbg /usr/local/bin/tbg
COPY --from=build /app/target/release/tbg-gateway /usr/local/bin/tbg-gateway
COPY LICENSE /usr/share/doc/telegram-bot-gateway/LICENSE
ENV HOME=/root
ENTRYPOINT ["tbg-gateway"]
