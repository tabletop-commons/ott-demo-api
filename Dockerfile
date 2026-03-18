# Multi-stage Dockerfile (per Deploying Guide "Container Image")
# Stage 1: Build with Rust toolchain
# Stage 2: Distroless runtime (ADR-0021)

# --- Build stage ---
FROM rust:1.93-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache dependency build
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# --- Runtime stage (distroless -- ADR-0021) ---
FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=build /app/target/release/ott-demo-api /app/ott-demo-api
EXPOSE 8080
ENTRYPOINT ["/app/ott-demo-api"]
