#!/usr/bin/env bash
set -euo pipefail

RPC_URL="${1:-http://0.0.0.0:3000/app/carneiro}"
RAW_TX="0xf86180808252089435353535353535353535353535353535353535358080820fd4a07288642ce140db4fb9c18694723feaf29214f18f277966b5aeb8e1bc33237a9ca07a6bf67e573cbf105a88afbd3d47ee8ec895d37a468995cbc5f28fd31939f24d"

curl \
  -sS \
  -X POST \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"eth_sendRawTransaction","params":["'""${RAW_TX}""'""]}' \
  "${RPC_URL}"
