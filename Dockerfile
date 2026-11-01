# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS build
ARG TARGETARCH
ARG TAILWIND_VERSION=4.1.11
WORKDIR /app

# Tailwind standalone (без Node)
RUN case "$TARGETARCH" in arm64) arch=arm64 ;; *) arch=x64 ;; esac \
 && curl -fsSL -o /usr/local/bin/tailwindcss \
    "https://github.com/tailwindlabs/tailwindcss/releases/download/v${TAILWIND_VERSION}/tailwindcss-linux-${arch}" \
 && chmod +x /usr/local/bin/tailwindcss

# Зависимостите се билдват отделно, за да се кешират
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY . .
# Тестовете спират билда, ако нещо е счупено, преди да стигне до сървъра.
RUN touch src/main.rs && cargo test --release --locked && cargo build --release --locked
RUN mkdir -p static && tailwindcss -i assets/app.css -o static/app.css --minify

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home /app app \
 && mkdir -p /data/uploads && chown -R app /data
WORKDIR /app
COPY --from=build /app/target/release/qr-manuals /usr/local/bin/qr-manuals
COPY --from=build /app/static ./static

ENV BIND_ADDR=0.0.0.0:8080 \
    DATABASE_URL=sqlite:///data/app.db \
    UPLOAD_DIR=/data/uploads \
    STATIC_DIR=/app/static
USER app
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["qr-manuals", "health"]
CMD ["qr-manuals", "serve"]
