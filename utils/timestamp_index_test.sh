#!/usr/bin/env bash
set -euo pipefail

# Timestamp index end-to-end test helper
# - Applies a reversible patch to toggle timestamp indexing on new blocks
# - Runs Stratus to mine blocks without indexing
# - Restores indexing, runs again on the same DB
# - Verifies timestamp RPC behavior (old=null, new=block)
# - Optionally migrates and re-verifies old timestamps

# Config (feel free to tweak)
PORT=3000
ROCKS_PREFIX="temp_ts_demo"               # DB directory will be "${ROCKS_PREFIX}-rocksdb"
BLOCK_MODE="1s"
MINE_SECONDS_WITHOUT_INDEX=12
VERIFY_NEW_SECONDS=6
RUN_MIGRATION="true"                      # set to "false" to skip migration step

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_FILE="src/eth/storage/permanent/rocks/rocks_state.rs"
DB_PATH="${REPO_ROOT}/${ROCKS_PREFIX}-rocksdb"

cd "${REPO_ROOT}"

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1"; exit 1; }
}

require_cmd git
require_cmd cargo
require_cmd just

info() { echo "[INFO] $*"; }
warn() { echo "[WARN] $*"; }

# Reversible patch that comments the two timestamp index insertions
PATCH_DISABLE=$(cat <<'PATCH'
diff --git a/src/eth/storage/permanent/rocks/rocks_state.rs b/src/eth/storage/permanent/rocks/rocks_state.rs
--- a/src/eth/storage/permanent/rocks/rocks_state.rs
+++ b/src/eth/storage/permanent/rocks/rocks_state.rs
@@ -462,8 +462,8 @@
         let block_by_hash = (block_hash.into(), number.into());
         self.blocks_by_hash.prepare_batch_insertion([block_by_hash], &mut batch)?;

-        let block_by_timestamp = (timestamp.into(), number.into());
-        self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch)?;
+        // let block_by_timestamp = (timestamp.into(), number.into());
+        // self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch)?;

         self.prepare_batch_with_execution_changes(account_changes, number, &mut batch)?;
@@ -510,8 +510,8 @@
         let block_by_hash = (block_hash.into(), number.into());
         self.blocks_by_hash.prepare_batch_insertion([block_by_hash], batch)?;

-        let block_by_timestamp = (timestamp.into(), number.into());
-        self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch)?;
+        // let block_by_timestamp = (timestamp.into(), number.into());
+        // self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch)?;

         self.prepare_batch_with_execution_changes(account_changes, number, batch)?;
PATCH
)

# Fallback patch using `patch` or Perl-based in-place replace
perl_disable() {
  info "Applying perl fallback to DISABLE timestamp indexing"
  /usr/bin/perl -0777 -pe '
    s{^([\t ]*)let\s+block_by_timestamp\s*=\s*\(timestamp\.into\(\),\s*number\.into\(\)\);\s*$}{$1// let block_by_timestamp = (timestamp.into(), number.into());}mg;
    s{^([\t ]*)self\.blocks_by_timestamp\.prepare_batch_insertion\(\[block_by_timestamp\],\s*&mut\s+batch\)\?;\s*$}{$1// self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch); }mg;
    s{^([\t ]*)self\.blocks_by_timestamp\.prepare_batch_insertion\(\[block_by_timestamp\],\s*batch\)\?;\s*$}{$1// self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch); }mg;
  ' -i "$TARGET_FILE"
}

perl_enable() {
  info "Applying perl fallback to ENABLE timestamp indexing"
  /usr/bin/perl -0777 -pe '
    s{^([\t ]*)//\s*let\s+block_by_timestamp\s*=\s*\(timestamp\.into\(\),\s*number\.into\(\)\);\s*$}{$1let block_by_timestamp = (timestamp.into(), number.into());}mg;
    s{^([\t ]*)//\s*self\.blocks_by_timestamp\.prepare_batch_insertion\(\[block_by_timestamp\],\s*&mut\s+batch\);\s*$}{$1self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch)?;}mg;
    s{^([\t ]*)//\s*self\.blocks_by_timestamp\.prepare_batch_insertion\(\[block_by_timestamp\],\s*batch\);\s*$}{$1self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch)?;}mg;
  ' -i "$TARGET_FILE"
}

apply_disable_patch() {
  info "Disabling timestamp indexing via patch"
  if command -v patch >/dev/null 2>&1; then
    if ! printf "%s\n" "$PATCH_DISABLE" | patch -p0 -N --dry-run >/dev/null 2>&1; then
      warn "patch --dry-run failed; trying git apply, then perl fallback"
      if ! echo "$PATCH_DISABLE" | git apply -p0 -v --cached --check --ignore-space-change --ignore-whitespace -; then
        perl_disable; return
      fi
      echo "$PATCH_DISABLE" | git apply -p0 -v --ignore-space-change --ignore-whitespace - || { perl_disable; }
    else
      printf "%s\n" "$PATCH_DISABLE" | patch -p0 -N || { warn "patch failed; using perl fallback"; perl_disable; }
    fi
  else
    warn "patch command not found; trying git apply, then perl fallback"
    if ! echo "$PATCH_DISABLE" | git apply -p0 -v --cached --check --ignore-space-change --ignore-whitespace -; then
      perl_disable; return
    fi
    echo "$PATCH_DISABLE" | git apply -p0 -v --ignore-space-change --ignore-whitespace - || { perl_disable; }
  fi
}

revert_disable_patch() {
  info "Re-enabling timestamp indexing via reverse patch"
  if command -v patch >/dev/null 2>&1; then
    if ! printf "%s\n" "$PATCH_DISABLE" | patch -p0 -R -N --dry-run >/dev/null 2>&1; then
      warn "patch -R --dry-run failed; trying git apply -R, then perl fallback"
      if ! echo "$PATCH_DISABLE" | git apply -p0 -R -v --cached --check --ignore-space-change --ignore-whitespace -; then
        perl_enable; return
      fi
      echo "$PATCH_DISABLE" | git apply -p0 -R -v --ignore-space-change --ignore-whitespace - || { perl_enable; }
    else
      printf "%s\n" "$PATCH_DISABLE" | patch -p0 -R -N || { warn "patch -R failed; using perl fallback"; perl_enable; }
    fi
  else
    warn "patch command not found; trying git apply -R, then perl fallback"
    if ! echo "$PATCH_DISABLE" | git apply -p0 -R -v --cached --check --ignore-space-change --ignore-whitespace -; then
      perl_enable; return
    fi
    echo "$PATCH_DISABLE" | git apply -p0 -R -v --ignore-space-change --ignore-whitespace - || { perl_enable; }
  fi
}

wait_up() {
  local tries=40
  while (( tries-- > 0 )); do
    if curl -s -m 1 http://localhost:${PORT} >/dev/null 2>&1; then return 0; fi
    sleep 0.5
  done
  return 1
}

rpc() {
  curl -s -X POST http://localhost:${PORT} -H 'content-type: application/json' --data "$1"
}

get_hex_ts_of_latest() {
  rpc '{"jsonrpc":"2.0","id":"latest","method":"eth_getBlockByNumber","params":["latest", false]}' | jq -r '.result.timestamp'
}

to_dec() {
  # printf handles 0x.. hex or decimal
  printf "%d" "$1"
}

query_by_ts() {
  local ts_dec="$1"
  rpc "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"stratus_getBlockByTimestamp\",\"params\":[${ts_dec}, false]}"
}

start_stratus() {
  info "Starting Stratus on port ${PORT} with rocks prefix '${ROCKS_PREFIX}'"
  RELEASE=1 just stratus-test -a 0.0.0.0:${PORT} --rocks-path-prefix=${ROCKS_PREFIX} --block-mode ${BLOCK_MODE} >/dev/null 2>&1 &
  if ! wait_up; then
    warn "Stratus didn't respond on port ${PORT} in time"
  fi
}

stop_stratus() {
  info "Stopping Stratus on port ${PORT}"
  if command -v killport >/dev/null 2>&1; then
    killport ${PORT} -s sigterm || true
  else
    pkill -f 'bin stratus' || true
  fi
  sleep 2
}

main() {
  info "TARGET_FILE: ${TARGET_FILE}"
  info "DB_PATH: ${DB_PATH}"

  # Phase 1: Disable indexing and generate blocks
  apply_disable_patch
  info "Building (first run, indexing disabled)"
  cargo build --quiet
  start_stratus
  info "Mining without timestamp index for ${MINE_SECONDS_WITHOUT_INDEX}s"
  sleep ${MINE_SECONDS_WITHOUT_INDEX}

  # Capture an OLD timestamp (hex -> dec) for later verification
  if command -v jq >/dev/null 2>&1; then
    HEX_TS_OLD=$(get_hex_ts_of_latest || echo "0x0")
    TS_OLD=$(to_dec "${HEX_TS_OLD}")
    info "Captured OLD timestamp (disabled phase): ${TS_OLD} (${HEX_TS_OLD})"
  else
    warn "jq not found; skipping OLD timestamp capture"
    TS_OLD=""
  fi

  stop_stratus

  # Phase 2: Re-enable indexing and verify new vs old
  revert_disable_patch
  info "Building (second run, indexing enabled)"
  cargo build --quiet
  start_stratus
  info "Waiting ${VERIFY_NEW_SECONDS}s to mine a couple of new blocks"
  sleep ${VERIFY_NEW_SECONDS}

  if command -v jq >/dev/null 2>&1; then
    HEX_TS_NEW=$(get_hex_ts_of_latest || echo "0x0")
    TS_NEW=$(to_dec "${HEX_TS_NEW}")
    info "Captured NEW timestamp (enabled phase): ${TS_NEW} (${HEX_TS_NEW})"

    info "Query by NEW timestamp (should return a block)"
    query_by_ts "${TS_NEW}" | jq . || query_by_ts "${TS_NEW}"

    if [[ -n "${TS_OLD}" ]]; then
      info "Query by OLD timestamp (should be null before migration)"
      query_by_ts "${TS_OLD}" | jq . || query_by_ts "${TS_OLD}"
    fi
  else
    warn "jq not found; skipping RPC verification"
  fi

  if [[ "${RUN_MIGRATION}" == "true" && -n "${TS_OLD}" ]]; then
    info "Running timestamp index migration on ${DB_PATH}"
    stop_stratus
    cargo run --release --bin migrate_timestamp_index -- --db-path "${DB_PATH}" --batch-size 50000
    start_stratus
    sleep 3
    info "Query by OLD timestamp after migration (should return a block)"
    query_by_ts "${TS_OLD}" | jq . || query_by_ts "${TS_OLD}"
  fi

  info "Done. DB kept at ${DB_PATH} for inspection."
}

main "$@"


