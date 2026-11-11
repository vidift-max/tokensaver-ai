FROM rust:1.86-bookworm AS builder
WORKDIR /workspace
COPY services/ai-team-room ./services/ai-team-room
RUN cargo build --release --manifest-path services/ai-team-room/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/services/ai-team-room/target/release/ai-team-room-service /usr/local/bin/ai-team-room-service
USER 10001:10001
ENV TEAM_ROOM_BIND=0.0.0.0:8081
EXPOSE 8081
ENTRYPOINT ["/usr/local/bin/ai-team-room-service"]
