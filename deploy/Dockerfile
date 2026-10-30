# Runtime image for the already-built Rust slice.
#
# This image deliberately does NOT compile the workspace. The audited release
# artifacts are produced by the native Cargo release build:
#
#   cargo build --release --locked -p archava-slice --bin archava-server
#   # site assets in target/site
#
# Building in Docker would silently use a different toolchain and different
# feature flags than the release proof. Build first, then:
#
#   docker build -f deploy/Dockerfile -t archava-runtime .
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 archava \
    && useradd --system --uid 10001 --gid archava \
        --home-dir /var/lib/archava --create-home --shell /usr/sbin/nologin archava

WORKDIR /opt/archava
COPY --chown=root:root target/release/archava-server /opt/archava/bin/archava-server
COPY --chown=root:root target/site /opt/archava/site

RUN chmod 0755 /opt/archava/bin/archava-server \
    && mkdir -p /var/lib/archava \
    && chown archava:archava /var/lib/archava

USER archava:archava
ENV ARCHAVA_BIND_ADDR=0.0.0.0:8347 \
    ARCHAVA_SITE_ROOT=/opt/archava/site \
    RUST_LOG=info

EXPOSE 8347
HEALTHCHECK --interval=10s --timeout=3s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8347/readyz || exit 1

ENTRYPOINT ["/opt/archava/bin/archava-server"]
