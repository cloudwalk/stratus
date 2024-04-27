#!/usr/bin/env bash

set -e

if [[ -z "$1" ]]; then
    echo >&2 "Provide the path for the data folder"
    exit 1
fi

if [[ -z "$2" ]]; then
    echo >&2 "Provide the start of the range"
    exit 1
fi

if [[ -z "$3" ]]; then
    echo >&2 "Provide the end of the range"
    exit 1
fi

DATA_FOLDER="$1"
RANGE_START="$2"
RANGE_END="$3"

files=$(ls -w 1 -v $DATA_FOLDER/*.csv)

function import() {
    file="$1"
    number="$2"

    echo "would import $file"

    echo "$file" >> imported.logs
}

for file in $files; do
    number=$(echo "$file" | grep -oP "\d+")

    # If in the expected range
    if [[ "$RANGE_START" -le "$number" && "$number" -lt "$RANGE_END" ]]; then
        import "$file" "$number"
    fi
done
