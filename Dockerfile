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
 && cargo build --release --locked

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /build/target/release/lifeping /usr/local/bin/lifeping
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/lifeping", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/lifeping"]
CMD ["serve"]
