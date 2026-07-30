#!/usr/bin/env bash
#
# Calls stratus_emitBlockEvents on each block in [START_BLOCK, END_BLOCK]
# to re-emit kafka events.
#
# Usage:
#   ./reemit_kafka_events.sh
#   ADMIN_PASSWORD=xxx ./reemit_kafka_events.sh
#   START_BLOCK=130981189 END_BLOCK=130986495 ./reemit_kafka_events.sh

set -uo pipefail

URL="${URL:-http://10.59.128.7:3000/app/kafka_reprocessing}"
START_BLOCK="${START_BLOCK:-130981188}"
END_BLOCK="${END_BLOCK:-130986495}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-}"

total=$((END_BLOCK - START_BLOCK + 1))
ok=0
fail=0

echo "Re-emitting kafka events for blocks ${START_BLOCK}..${END_BLOCK} (${total} blocks)"
echo "Target: ${URL}"
echo

for ((block = START_BLOCK; block <= END_BLOCK; block++)); do
    hex_block=$(printf '0x%x' "$block")
    payload=$(printf '{"jsonrpc":"2.0","id":1,"method":"stratus_emitBlockEvents","params":["%s"]}' "$hex_block")

    if [[ -n "$ADMIN_PASSWORD" ]]; then
        response=$(curl -sS -X POST "$URL" \
            -H "Content-Type: application/json" \
            -H "Authorization: Password ${ADMIN_PASSWORD}" \
            -d "$payload")
    else
        response=$(curl -sS -X POST "$URL" \
            -H "Content-Type: application/json" \
            -d "$payload")
    fi

    if echo "$response" | grep -q '"error"'; then
        fail=$((fail + 1))
        echo "block ${block} (${hex_block}): FAIL ${response}"
    else
        ok=$((ok + 1))
    fi

    processed=$((block - START_BLOCK + 1))
    if ((processed % 100 == 0)); then
        echo "  progress: ${processed}/${total} (ok=${ok} fail=${fail})"
    fi
done

echo
echo "Done. ok=${ok} fail=${fail} total=${total}"
