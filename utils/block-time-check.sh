#!/bin/bash

set -e

sleep_interval=120
error_margin=0.05

echo -n "-> Waiting for blocks to generate... "
for ((i=1; i<=$sleep_interval; i++)); do
    printf "\r-> Waiting for blocks to generate... %d/%ds" $i $sleep_interval
    sleep 1
done

echo "-> Getting info on first block"
first_block_info=$(curl -s http://0.0.0.0:3000 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["earliest",false],"id":1}')

echo "-> Getting info on latest block"
latest_block_info=$(curl -s http://0.0.0.0:3000 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}')

# Extract block time and block number
first_block_time_hex=$(echo ${first_block_info} | jq -r '.result.timestamp')
latest_block_time_hex=$(echo ${latest_block_info} | jq -r '.result.timestamp')
block_count_hex=$(echo ${latest_block_info} | jq -r '.result.number')

# Remove 0x prefix
first_block_time_hex_no_prefix=${first_block_time_hex#0x}
latest_block_time_hex_no_prefix=${latest_block_time_hex#0x}
block_count_hex_no_prefix=${block_count_hex#0x}

# Convert hex to number
first_block_time_dec=$((16#${first_block_time_hex_no_prefix}))
latest_block_time_dec=$((16#${latest_block_time_hex_no_prefix}))  
block_count=$((16#${block_count_hex_no_prefix}))

# Elapsed time between first and latest block
block_time_elapsed=$(($latest_block_time_dec - $first_block_time_dec))

# Amount of blocks mined during the elapsed time
blocks_mined=$((block_count - 1))

# Average block generation time
average_block_time=$(echo "scale=2; $block_time_elapsed / $blocks_mined" | bc)
echo "-> Average block generation time: $average_block_time s"

# Calculate the absolute value of the difference between the average block time and the expected block time
actual_error_margin=$(echo "$average_block_time - 1" | bc)
if (( $(echo "$actual_error_margin < 0" | bc -l) )); then
    actual_error_margin=$(echo "$actual_error_margin * -1" | bc)
fi

# Check if the difference is within the acceptable range
if (( $(echo "$actual_error_margin <= $error_margin" | bc -l) )); then
    echo "Average block time is within the acceptable range: $average_block_time per second"
    exit 0
fi
echo "Error: Average block time is not within the acceptable range"
exit 1