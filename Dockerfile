# syntax=docker/dockerfile:1

FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build

# Cache dependencies: build once against stub sources.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
 && echo 'fn main() {}' > src/main.rs \
 && touch src/lib.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY src ./src
COPY web ./web
# Make sure cargo notices the real sources are newer than the stub build.
RUN touch src/main.rs src/lib.rs \
 && cargo build --release --locked \
 && mkdir /build/data

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /build/target/release/lifeping /usr/local/bin/lifeping
# Distroless has no shell to chown with, so ship an empty /data owned by
# nonroot. A new named volume inherits this ownership; without it, VOLUME
# would create /data as root and the server couldn't write to it.
COPY --from=builder --chown=65532:65532 /build/data /data
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/lifeping", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/lifeping"]
CMD ["serve"]
