FROM ubuntu:24.04

ARG RUST_VERSION=1.98.1
ARG ZORE_VERSION=0.1.0-alpha.1
ARG ZORE_REVISION
ARG ZORE_SOURCE=https://github.com/maasdi/zore

LABEL org.opencontainers.image.title="Zore builder" \
      org.opencontainers.image.version="${ZORE_VERSION}" \
      org.opencontainers.image.revision="${ZORE_REVISION}" \
      org.opencontainers.image.source="${ZORE_SOURCE}" \
      org.opencontainers.image.licenses="Apache-2.0"

ENV CARGO_HOME=/opt/cargo \
    RUSTUP_HOME=/opt/rust \
    RUSTUP_TOOLCHAIN=${RUST_VERSION} \
    ZORE_RUSTC=/opt/rust/toolchains/${RUST_VERSION}-x86_64-unknown-linux-gnu/bin/rustc \
    ZORE_CC=/usr/bin/clang \
    PATH=/opt/cargo/bin:${PATH}

RUN test "$(dpkg --print-architecture)" = amd64 \
    && apt-get update \
    && apt-get install -y --no-install-recommends clang build-essential ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o /tmp/rustup-init.sh \
    && sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain "${RUST_VERSION}" --no-modify-path \
    && rm /tmp/rustup-init.sh \
    && "${ZORE_RUSTC}" --version

COPY --chmod=0755 zore /usr/local/bin/zore

RUN test "$(zore --version)" = "zore ${ZORE_VERSION}"

WORKDIR /workspace
CMD ["zore", "--help"]
