FROM rust:1.85-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM node:20-slim AS frontend
WORKDIR /app
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ .
RUN npm run build

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/iris-server .
COPY --from=frontend /app/dist ./web/dist
COPY migrations/ migrations/
ENV PORT=3000
EXPOSE 3000
CMD ["./iris-server"]
