# Build
FROM rust:1.75 as builder

WORKDIR /app
COPY src /app/src
COPY static /app/static
COPY .sqlx /app/.sqlx
COPY build.rs /app/build.rs
COPY Cargo.toml /app/Cargo.toml
COPY Cargo.lock /app/Cargo.lock

RUN apt update
RUN apt-get install -y libclang-dev cmake

RUN cargo build --release --features metrics

# Runtime
FROM rust:1.75 as runtime
WORKDIR /app
COPY --from=builder /app/target/release/stratus /app/stratus

CMD ["sh", "-c", "/app/stratus"]
