#!/usr/bin/env bash
#
# Script to monitor and report the progress of a csv exporter run
set -e

RED='\033[0;31m'
RESET='\033[0m'

if [[ -t 1 ]]; then
    echo >&2 "Tip: run this with 'tee' to save output in another file."
    echo >&2 ""
fi

PID=$1
if [[ "$PID" == "" ]]; then
    echo >&2 "ERROR: importer-offline process PID must be provided."
    exit 1
fi

if [[ "$(basename $(pwd))" != "utils" ]]; then
    echo >&2 "WARNING: this script it supposed to be run inside of the"
    echo >&2 "  utils folder, and might fail otherwise"
    echo >&2 ""
    read -p  "  Press enter if you want to continue..."
    echo >&2 "  OK, continuing."
fi

function machine_amount_of_free_memory_gb() {
    awk '/MemFree/ { printf "%.1f\n", $2/1024/1024 }' /proc/meminfo
}


# A rough estimate of the current total amount of transactions
TOTAL_TRANSACTIONS=320100100

minutes=0
previous_transaction=""

while ps -p $PID > /dev/null; do
    process_mem_percentage=$(ps -p "$PID" -o %mem | tail -n 1 | tr -d "\n ")
    current_block=$(cat ../data/blocks-last-id.txt)
    current_transaction=$(cat ../data/transactions-last-id.txt)
    transaction_progress=$(echo "scale=1; $current_transaction * 100.0 / $TOTAL_TRANSACTIONS" | bc)

    if [[ "$previous_transaction" == "" ]]; then
        tps="N/A"
        eta_h="N/A"
    else
        tps=$(( (current_transaction - previous_transaction) / 60 ))
        remaining_transactions=$(( TOTAL_TRANSACTIONS - current_transaction ))
        if [[ "$tps" == 0 ]]; then
            tps=1
        fi
        eta_h=$(( (remaining_transactions / tps) / 60 / 60 ))
    fi

    free_memory_amount=$(machine_amount_of_free_memory_gb)
    log="mins_elapsed = $minutes, free_memory = $free_memory_amount GB, importer-off_mem% = $process_mem_percentage%, block = $current_block, transaction = $current_transaction ($transaction_progress%), tps = $tps, ETA=$eta_h hours"
    echo "$log"

    # If free memory is below 10GB
    if [[ "$(echo $free_memory_amount | cut -d . -f 1)" -lt 10 ]]; then
        echo -e "${RED}WARNING: FREE MEMORY BELOW 10GB${RESET}"
    fi

    previous_transaction="$current_transaction"
    minutes=$(( minutes + 1 ))
    sleep 60
done

echo "Stopping. Monitored process $PID ended."
