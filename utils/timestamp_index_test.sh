#!/usr/bin/env bash
set -euo pipefail

# Minimal timestamp index test:
# - Apply an inline git patch to comment out timestamp index writes
# - Start Stratus for a short time (mining without timestamp index)
# - Stop Stratus, discard the patch (restore file)
# - Start Stratus again on the same DB (now indexing is enabled)
# - Verify:
#   * latest block by timestamp works
#   * early block (e.g., #1) by timestamp fails before migration
#   * run migration, then early block by timestamp works

PORT=3000
ROCKS_PREFIX="temp_ts_demo"
BLOCK_MODE="1s"
MINE_SECONDS=10

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_FILE="src/eth/storage/permanent/rocks/rocks_state.rs"
DB_PATH="${REPO_ROOT}/${ROCKS_PREFIX}-rocksdb"

cd "${REPO_ROOT}"

info() { echo "[INFO] $*"; }

rpc() {
  curl -s -X POST http://localhost:${PORT} -H 'content-type: application/json' --data "$1"
}

extract_json_field() {
  local json="$1"; local field="$2"
  echo "$json" | jq -r ".result.${field}"
}

to_dec() {
  # Converts 0x... or decimal to decimal
  printf "%d" "$1"
}

# Inline unified diff that comments out the two timestamp index writes
read -r -d '' DISABLE_PATCH <<'PATCH'
diff --git a/src/eth/storage/permanent/rocks/rocks_state.rs b/src/eth/storage/permanent/rocks/rocks_state.rs
--- a/src/eth/storage/permanent/rocks/rocks_state.rs
+++ b/src/eth/storage/permanent/rocks/rocks_state.rs
@@ -462,12 +462,12 @@
         let block_by_hash = (block_hash.into(), number.into());
         self.blocks_by_hash.prepare_batch_insertion([block_by_hash], &mut batch)?;

-        let block_by_timestamp = (timestamp.into(), number.into());
-        self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch)?;
+        // let block_by_timestamp = (timestamp.into(), number.into());
+        // self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], &mut batch)?;

         self.prepare_batch_with_execution_changes(account_changes, number, &mut batch)?;
@@ -510,12 +510,12 @@
         let block_by_hash = (block_hash.into(), number.into());
         self.blocks_by_hash.prepare_batch_insertion([block_by_hash], batch)?;

-        let block_by_timestamp = (timestamp.into(), number.into());
-        self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch)?;
+        // let block_by_timestamp = (timestamp.into(), number.into());
+        // self.blocks_by_timestamp.prepare_batch_insertion([block_by_timestamp], batch)?;

         self.prepare_batch_with_execution_changes(account_changes, number, batch)?;
PATCH

apply_patch() {
  info "Applying disable patch with git"
  echo "${DISABLE_PATCH}" | git apply -p0 --check
  echo "${DISABLE_PATCH}" | git apply -p0
}

revert_file() {
  info "Discarding changes to ${TARGET_FILE}"
  git checkout -- "${TARGET_FILE}"
}

start_stratus() {
  info "Starting Stratus on :${PORT} with rocks prefix '${ROCKS_PREFIX}'"
  RELEASE=1 just stratus-test -a 0.0.0.0:${PORT} --rocks-path-prefix=${ROCKS_PREFIX} --block-mode ${BLOCK_MODE} >/dev/null 2>&1 &
  for _ in $(seq 1 40); do
    curl -s -m 1 http://localhost:${PORT} >/dev/null 2>&1 && return 0 || sleep 0.25
  done
  info "Stratus startup wait finished"
}

stop_stratus() {
  info "Stopping Stratus on :${PORT}"
  if command -v killport >/dev/null 2>&1; then
    killport ${PORT} -s sigterm || true
  else
    pkill -f 'bin stratus' || true
  fi
  sleep 2
}

main() {
  info "Repo: ${REPO_ROOT}"
  info "DB:   ${DB_PATH}"

  # Phase 1: Disable indexing; run a bit
  apply_patch
  cargo build --quiet
  start_stratus
  info "Mining without timestamp index for ${MINE_SECONDS}s"
  sleep "${MINE_SECONDS}"
  stop_stratus

  # Phase 2: Restore file; run again on same DB (indexing enabled)
  revert_file
  cargo build --quiet
  start_stratus
  info "Mining with indexing enabled for ${MINE_SECONDS}s"
  sleep "${MINE_SECONDS}"

  # Verification: latest
  LATEST_JSON=$(rpc '{"jsonrpc":"2.0","id":"latest","method":"eth_getBlockByNumber","params":["latest", false]}')
  TS_NEW_HEX=$(echo "$LATEST_JSON" | jq -r ".result.timestamp")
  TS_NEW_DEC=$(to_dec "${TS_NEW_HEX:-0}")
  info "Latest timestamp: ${TS_NEW_DEC} (${TS_NEW_HEX})"
  RES_NEW=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"stratus_getBlockByTimestamp\",\"params\":[${TS_NEW_DEC}, false]}")
  if command -v jq >/dev/null 2>&1; then echo "$RES_NEW" | jq .; else echo "$RES_NEW"; fi

  # Verification: early block (block #1)
  EARLY_JSON=$(rpc '{"jsonrpc":"2.0","id":"earliest","method":"eth_getBlockByNumber","params":["0x1", false]}')
  TS_OLD_HEX=$(echo "$EARLY_JSON" | jq -r ".result.timestamp")
  TS_OLD_DEC=$(to_dec "${TS_OLD_HEX:-0}")
  info "Early block #1 timestamp: ${TS_OLD_DEC} (${TS_OLD_HEX})"
  RES_OLD_BEFORE=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"stratus_getBlockByTimestamp\",\"params\":[${TS_OLD_DEC}, false]}")
  info "Early by timestamp BEFORE migration (expect null):"
  if command -v jq >/dev/null 2>&1; then echo "$RES_OLD_BEFORE" | jq .; else echo "$RES_OLD_BEFORE"; fi

  # Migration
  info "Running migrate_timestamp_index on ${DB_PATH}"
  stop_stratus
  cargo run --release --bin migrate_timestamp_index -- --db-path "${DB_PATH}" --batch-size 50000
  start_stratus
  sleep 2

  # Verify old again
  RES_OLD_AFTER=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"stratus_getBlockByTimestamp\",\"params\":[${TS_OLD_DEC}, false]}")
  info "Early by timestamp AFTER migration (expect block):"
  if command -v jq >/dev/null 2>&1; then echo "$RES_OLD_AFTER" | jq .; else echo "$RES_OLD_AFTER"; fi

  info "Done. DB kept at ${DB_PATH}"
}

main "$@"