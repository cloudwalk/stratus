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

RUN cargo build --release

# Runtime
FROM rust:1.75 as runtime
WORKDIR /app
COPY --from=builder /app/target/release/stratus /app/stratus

CMD ["sh", "-c", "/app/stratus", "--pem-storage=$PERM_STORAGE", "--perm-storage-connections=$PERM_STORAGE_CONNECTIONS", "--perm-storage-timeout=$PERM_STORAGE_TIMEOUT", "--chain-id=$CHAIN_ID", "--blocking-threads=$BLOCKING_THREADS", "--temp-storage=$TEMP_STORAGE_KIND", "--evms=$NUM_EVMS"]
