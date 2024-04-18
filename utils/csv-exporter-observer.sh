#!/usr/bin/env bash
#
# Script to monitor and report the progress of a csv exporter run
set -e


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


# A rough estimate of the current total amount of transactions 
TOTAL_TRANSACTIONS=304100100

minutes=0
previous_transaction=""

while ps -p $PID > /dev/null; do
    mem_percentage=$(ps -p "$PID" -o %mem | tail -n 1 | tr -d "\n ")
    current_block=$(cat ../data/blocks-last-id.txt)
    current_transaction=$(cat ../data/transactions-last-id.txt)

    if [[ "$previous_transaction" == "" ]]; then
        tps="N/A"
        eta_h="N/A"
    else
        tps=$(( (current_transaction - previous_transaction) / 60 ))
        remaining_transactions=$(( TOTAL_TRANSACTIONS - current_transaction ))
        eta_h=$(( (remaining_transactions / tps) / 60 / 60 ))
    fi

    log="minutes_elapsed = $minutes, mem% = $mem_percentage, block = $current_block, transaction = $current_transaction, tps = $tps, ETA=$eta_h hours"
    echo "$log"

    previous_transaction="$current_transaction"
    minutes=$(( minutes + 1 ))
    sleep 60
done

echo "Stopping. Monitored process $PID ended."
