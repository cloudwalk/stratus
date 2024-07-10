#!/bin/bash

# Function to read log files and store entries in an array
read_logs() {
  local file=$1
  jq -c '.[]' "$file"
}

# Function to compare logs and find the common index
compare_logs() {
  local log_files=("$@")
  local common_index=0
  local total_entries=0
  local max_entries=0

  # Read the first log file into an array
  local base_log
  base_log=($(read_logs "${log_files[0]}"))
  total_entries=${#base_log[@]}

  # Find the maximum length of the log files
  for file in "${log_files[@]}"; do
    local log_length
    log_length=$(jq 'length' "$file")
    if (( log_length > max_entries )); then
      max_entries=$log_length
    fi
  done

  # Loop through the indices of the base log
  for ((i=0; i<${#base_log[@]}; i++)); do
    local entry=${base_log[i]}
    local all_match=true

    # Compare with other log files
    for file in "${log_files[@]:1}"; do
      local other_entry
      other_entry=$(jq -c ".[$i]" "$file")
      if [[ "$entry" != "$other_entry" ]]; then
        all_match=false
        break
      fi
    done

    if $all_match; then
      common_index=$i
    else
      break
    fi
  done

  echo "Common entries up to index: $common_index"

  local different_entries=$((max_entries - common_index - 1))
  local difference_percentage=$((invalid_entries * 100 / max_entries))

  echo "Different entries: $different_entries out of $max_entries"
  echo "Difference percentage: $difference_percentage%"

  if (( difference_percentage > 5 )); then
    echo "Error: Different log entries exceed the threshold"
    exit 1
  fi
}

# Function to validate indices and terms
validate_logs() {
  local file=$1

  local indices
  local terms
  indices=$(jq '.[].index' "$file")
  terms=$(jq '.[].term' "$file")

  local prev_index=0
  local prev_term=0
  local index_errors=false
  local term_errors=false

  while IFS= read -r index; do
    if (( index != prev_index + 1 )); then
      echo "Index error at: $index"
      index_errors=true
    fi
    prev_index=$index
  done <<< "$indices"

  while IFS= read -r term; do
    if (( term < prev_term )); then
      echo "Term error at: $term"
      term_errors=true
    fi
    prev_term=$term
  done <<< "$terms"

  if ! $index_errors; then
    echo "Indices are correctly incremented"
  fi
  if ! $term_errors; then
    echo "Terms are correctly incremented"
  fi
}

# Main script
log_files=("$@")

if (( ${#log_files[@]} < 1 )); then
  echo "Please provide at least one log file."
  exit 1
fi

if (( ${#log_files[@]} > 1 )); then
  compare_logs "${log_files[@]}"
else
  echo "Skipping comparison as only one log file is provided."
fi

for file in "${log_files[@]}"; do
  echo "Validating log file: $file"
  validate_logs "$file"
done
