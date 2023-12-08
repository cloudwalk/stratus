# Runs the service locally locally
run:
    cargo run

# Compile project with debug options
build:
    cargo build

# Compile project with release options
build-release:
    cargo build --release

# Clean project build directory
clean:
    cargo clean

# Compile SQLx queries
sqlx:
    SQLX_OFFLINE=true cargo sqlx prepare --database-url postgres://postgres:123@0.0.0.0:5432/ledger -- --all-targets

# Execute all tests
test name="":
    @just test-doc {{name}}
    @just test-unit {{name}}
    @just test-int {{name}}

# Execute doc tests
test-doc name="":
    cargo +nightly test {{name}} --doc

# Execute unit tests
test-unit name="":
    cargo test --lib {{name}} -- --nocapture

# Execute integration tests
test-int name="":
    cargo test --test '*' {{name}} -- --nocapture

# Generate documentation for all crates
doc:
    @just test-doc
    cargo +nightly doc --no-deps

# Format code and run configured linters
lint:
    cargo +nightly fmt --all && cargo +nightly clippy --all-targets
