# Rust on Linux (amd64, or arm64 for benchmark.py on Apple Silicon), in two targets:
#   cli        the scrimmage command (platform_check.py)
#   benchmark  the core-only runner inside the C++ reference image (benchmark.py)
ARG REFERENCE_IMAGE=scrimmage-rs-reference:latest

FROM rust:1.98-slim-bookworm AS build
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY reference/benchmark_runner.rs ./reference/benchmark_runner.rs
RUN cargo build --locked --release --package scrimmage-rs --bin scrimmage \
    && cp target/release/scrimmage /usr/local/bin/scrimmage
RUN cargo bench --locked -p scrimmage-core --bench reference --no-run \
    && find target/release/deps -maxdepth 1 -type f -executable -name 'reference-*' \
       -exec cp {} /usr/local/bin/scrimmage-benchmark \;
RUN rustc --version --verbose > /rust-compiler.txt

FROM build AS cli
ENTRYPOINT ["/usr/local/bin/scrimmage"]

FROM ${REFERENCE_IMAGE} AS benchmark
# benchmark.py deletes earlier images with this label after each build.
LABEL scrimmage-rs-benchmark=1
COPY --from=build /usr/local/bin/scrimmage-benchmark /usr/local/bin/scrimmage-benchmark
COPY --from=build /rust-compiler.txt /rust-compiler.txt
# GNU time reports each simulator's own peak memory; a Python parent would inflate it.
RUN apt-get update && apt-get install -y --no-install-recommends time && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["python3", "/rust/reference/benchmark.py", "--inside"]
