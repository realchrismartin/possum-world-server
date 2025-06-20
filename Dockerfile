# Build stage
FROM rust:1.84-alpine AS builder
WORKDIR /app
COPY . .
RUN apk add --no-cache musl-dev
RUN cargo build --release

# Runtime stage
FROM alpine:3.19
WORKDIR /app
COPY --from=builder /app/target/release/possum-world-server /app/possum-world-server
COPY --from=builder /app/Cargo.toml /app/Cargo.toml
COPY --from=builder /app/README.md /app/README.md
RUN useradd -m appuser && chown appuser /app/possum-world-server
USER appuser

CMD ["/app/possum-world-server"]
