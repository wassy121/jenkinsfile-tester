# ── Stage 1: Build WASM + collect wasmtime runtime ───────────────────────────
FROM alpine:3.21 AS builder

RUN echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" >> /etc/apk/repositories && \
    apk add --no-cache gcc musl-dev wget wasmtime

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN wget -qO- https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.73.0 && \
    export PATH="$HOME/.cargo/bin:$PATH" && \
    rustup target add wasm32-wasi && \
    cargo build --bin jenkinsfile-tester --target wasm32-wasi --release

# Collect wasmtime and every .so it needs
RUN mkdir -p /collect/lib /collect/usr/bin && \
    cp /usr/bin/wasmtime /collect/usr/bin/wasmtime && \
    for lib in $(ldd /usr/bin/wasmtime | awk '/=>/ {print $3}' | sort -u); do \
        cp "$lib" /collect/lib/; \
    done && \
    cp /lib/ld-musl-*.so.1 /collect/lib/

# ── Stage 2: FROM scratch — only wasmtime, its libs, and the .wasm ───────────
FROM scratch

COPY --from=builder /collect/lib/         /lib/
COPY --from=builder /collect/usr/bin/wasmtime /usr/bin/wasmtime
COPY --from=builder /src/target/wasm32-wasi/release/jenkinsfile-tester.wasm /jenkinsfile-tester.wasm

# Usage:
#   docker run --rm -i jenkinsfile-tester validate < Jenkinsfile
#   docker run --rm -i jenkinsfile-tester validate-strict < Jenkinsfile
#   docker run --rm -i jenkinsfile-tester run-tests < Jenkinsfile
#   docker run --rm -i jenkinsfile-tester parse < Jenkinsfile
#   docker run --rm    jenkinsfile-tester dump-registry
#
# Custom plugin registry (mount the file and grant WASI access):
#   docker run --rm -i -v ./my-plugins.json:/registry.json \
#     jenkinsfile-tester --dir=/ --registry /registry.json validate < Jenkinsfile
ENV HOME=/tmp
ENTRYPOINT ["wasmtime", "run", "/jenkinsfile-tester.wasm", "--"]
CMD ["validate"]
