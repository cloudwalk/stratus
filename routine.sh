#!/bin/bash
# Script to download stratus and rocks_migrator binaries from GitHub workflow artifacts
# for a list of branches

set -e

# Configuration
REPO_OWNER="CloudWalk"
REPO_NAME="stratus"
WORKFLOW_NAME="Build Binary"
WORKFLOW_ID="build-binary.yml"
DESTINATION_DIR="/usr/local/stratus/bin"
GITHUB_TOKEN=${GITHUB_TOKEN:-""}  # Set your GitHub token as an environment variable
MAX_WAIT_TIME=1800  # Maximum time to wait for workflow to complete (in seconds)
POLL_INTERVAL=30    # Time between checks for workflow completion (in seconds)

# Routine test configuration
SERVER_ENDPOINT=${SERVER_ENDPOINT:-"http://10.52.184.7:3232"}
STATUS_ENDPOINT="${SERVER_ENDPOINT}/status"
CASHIER_CONTRACT=${CASHIER_CONTRACT:-"0x6ac607aBA84f672C092838a5c32c22907765F666"}
PRIVATE_KEYS_FILE=${PRIVATE_KEYS_FILE:-"./keys_new.txt"}
LEADER_HTTP_ADDRESS=${LEADER_HTTP_ADDRESS:-"http://10.52.184.6:3000/?app=bench"}
RUN_DURATION=${RUN_DURATION:-300}  # Duration in seconds
STATUS_CHECK_INTERVAL=60  # Time between status checks (in seconds)

# Check if GitHub token is provided
if [ -z "$GITHUB_TOKEN" ]; then
  echo "Warning: GITHUB_TOKEN environment variable not set. API rate limits may apply."
  AUTH_HEADER=""
else
  AUTH_HEADER="Authorization: token $GITHUB_TOKEN"
fi

# Create destination directory if it doesn't exist
mkdir -p "$DESTINATION_DIR"

# Function to check if a branch has a successful build-binary workflow run
check_workflow_run() {
  local branch=$1
  
  echo "Checking workflow runs for branch: $branch"
  
  # Get the latest workflow run for the branch
  local workflow_runs=$(curl -s -H "$AUTH_HEADER" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/runs?branch=$branch&status=success&per_page=1")
  
  # Check if there are any successful workflow runs
  local total_count=$(echo "$workflow_runs" | jq '.total_count')
  
  if [ "$total_count" -eq 0 ]; then
    echo "No successful workflow runs found for branch: $branch"
    return 1
  fi
  
  # Get the workflow run ID
  local run_id=$(echo "$workflow_runs" | jq '.workflow_runs[0].id')
  
  echo "Found successful workflow run (ID: $run_id) for branch: $branch"
  return 0
}

# Function to trigger a workflow run
trigger_workflow_run() {
  local branch=$1
  
  echo "Triggering workflow run for branch: $branch"
  
  # Trigger the workflow and capture HTTP status code
  local http_code=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
    -H "$AUTH_HEADER" \
    -H "Accept: application/vnd.github.v3+json" \
    -H "Content-Type: application/json" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/dispatches" \
    -d "{\"ref\":\"$branch\", \"inputs\": {\"features\": \"default\"}}")
  
  echo "GitHub API response code: $http_code"
  
  # Check if the request was successful (GitHub returns 204 No Content on success)
  if [ "$http_code" -ne 204 ]; then
    echo "Failed to trigger workflow for branch: $branch (HTTP code: $http_code)"
    
    # For debugging, let's make a verbose request to see the error
    echo "Detailed error response:"
    curl -v -X POST \
      -H "$AUTH_HEADER" \
      -H "Accept: application/vnd.github.v3+json" \
      -H "Content-Type: application/json" \
      "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/dispatches" \
      -d "{\"ref\":\"$branch\", \"inputs\": {\"features\": \"default\"}}"
    
    return 1
  fi
  
  # Verify that the workflow was actually triggered by checking for a new run
  echo "Verifying workflow was triggered..."
  sleep 5  # Give GitHub a moment to register the new workflow run
  
  local before_runs=$(curl -s -H "$AUTH_HEADER" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/runs?branch=$branch&per_page=1" | \
    jq '.total_count')
  
  if [ "$before_runs" -eq 0 ]; then
    echo "No workflow runs found after triggering. The workflow may not have been triggered successfully."
    return 1
  fi
  
  echo "Successfully triggered workflow for branch: $branch"
  return 0
}

# Function to wait for a workflow run to complete
wait_for_workflow_run() {
  local branch=$1
  local start_time=$(date +%s)
  
  echo "Waiting for workflow run to complete for branch: $branch"
  
  # Get the initial run ID to track
  local initial_runs=$(curl -s -H "$AUTH_HEADER" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/runs?branch=$branch&per_page=1")
  
  local initial_count=$(echo "$initial_runs" | jq '.total_count')
  
  if [ "$initial_count" -eq 0 ]; then
    echo "No workflow runs found initially for branch: $branch. Will wait for one to appear..."
  else
    local initial_run_id=$(echo "$initial_runs" | jq '.workflow_runs[0].id')
    echo "Found initial workflow run with ID: $initial_run_id"
  fi
  
  while true; do
    # Check if we've exceeded the maximum wait time
    local current_time=$(date +%s)
    local elapsed_time=$((current_time - start_time))
    
    if [ $elapsed_time -gt $MAX_WAIT_TIME ]; then
      echo "Timeout waiting for workflow run to complete for branch: $branch"
      return 1
    fi
    
    # Get the latest workflow run for the branch
    local workflow_runs=$(curl -s -H "$AUTH_HEADER" \
      "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/runs?branch=$branch&per_page=1")
    
    # Check if there are any workflow runs
    local total_count=$(echo "$workflow_runs" | jq '.total_count')
    
    if [ "$total_count" -eq 0 ]; then
      echo "No workflow runs found for branch: $branch. Waiting..."
      sleep $POLL_INTERVAL
      continue
    fi
    
    # Get the workflow run status
    local status=$(echo "$workflow_runs" | jq -r '.workflow_runs[0].status')
    local conclusion=$(echo "$workflow_runs" | jq -r '.workflow_runs[0].conclusion')
    local run_id=$(echo "$workflow_runs" | jq '.workflow_runs[0].id')
    local html_url=$(echo "$workflow_runs" | jq -r '.workflow_runs[0].html_url')
    
    echo "Workflow run status: $status, conclusion: $conclusion (ID: $run_id)"
    echo "Workflow URL: $html_url"
    
    # Check if the workflow run is complete
    if [ "$status" = "completed" ]; then
      if [ "$conclusion" = "success" ]; then
        echo "Workflow run completed successfully for branch: $branch"
        return 0
      else
        echo "Workflow run failed for branch: $branch (conclusion: $conclusion)"
        echo "Check the workflow logs at: $html_url"
        return 1
      fi
    fi
    
    # Calculate and display the elapsed time
    local minutes=$((elapsed_time / 60))
    local seconds=$((elapsed_time % 60))
    echo "Workflow run still in progress. Elapsed time: ${minutes}m ${seconds}s. Waiting..."
    sleep $POLL_INTERVAL
  done
}

# Function to download artifacts from a workflow run
download_artifacts() {
  local branch=$1
  
  echo "Downloading artifacts for branch: $branch"
  
  # Get the latest workflow run for the branch
  local workflow_runs=$(curl -s -H "$AUTH_HEADER" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/workflows/$WORKFLOW_ID/runs?branch=$branch&status=success&per_page=1")
  
  local run_id=$(echo "$workflow_runs" | jq '.workflow_runs[0].id')
  
  # Get artifacts for the workflow run
  local artifacts=$(curl -s -H "$AUTH_HEADER" \
    "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/runs/$run_id/artifacts")
  
  # Extract artifact IDs and names
  local artifact_count=$(echo "$artifacts" | jq '.total_count')
  
  echo "Found $artifact_count artifacts for workflow run: $run_id"
  
  # Download each artifact
  for i in $(seq 0 $(($artifact_count - 1))); do
    local artifact_name=$(echo "$artifacts" | jq -r ".artifacts[$i].name")
    local artifact_id=$(echo "$artifacts" | jq -r ".artifacts[$i].id")
    
    # Only download stratus and rocks_migrator artifacts
    if [[ "$artifact_name" == *"stratus"* ]] || [[ "$artifact_name" == *"rocks_migrator"* ]]; then
      echo "Downloading artifact: $artifact_name (ID: $artifact_id)"
      
      # Create a temporary directory for the download
      local temp_dir=$(mktemp -d)
      
      # Download the artifact
      curl -s -L -H "$AUTH_HEADER" \
        "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/actions/artifacts/$artifact_id/zip" \
        -o "$temp_dir/artifact.zip"
      
      # Extract the artifact
      unzip -q "$temp_dir/artifact.zip" -d "$temp_dir"
      
      # Determine the binary name based on the artifact name
      local binary_name=""
      if [[ "$artifact_name" == *"stratus"* ]]; then
        binary_name="stratus"
      elif [[ "$artifact_name" == *"rocks_migrator"* ]]; then
        binary_name="rocks_migrator"
      fi
      
      if [ -n "$binary_name" ]; then
        # Find the binary file which might have a commit hash appended
        local binary_file=$(find "$temp_dir" -type f -name "${binary_name}*" | head -n 1)
        
        if [ -n "$binary_file" ]; then
          # Stop stratus service before copying if this is the stratus binary
          if [[ "$binary_name" == "stratus" ]]; then
            echo "Stopping stratus service before replacing binary..."
            sudo systemctl stop stratus
            # Give it a moment to fully stop
            sleep 2
          fi
          
          # Copy the binary to the destination directory with just the base name
          cp "$binary_file" "$DESTINATION_DIR/$binary_name"
          chmod +x "$DESTINATION_DIR/$binary_name"
          echo "Installed $binary_name from branch $branch to $DESTINATION_DIR/$binary_name"
        else
          echo "Error: Could not find binary file for $binary_name in the extracted artifact"
          ls -la "$temp_dir"  # List files for debugging
          return 1
        fi
      fi
      
      # Clean up
      rm -rf "$temp_dir"
    fi
  done
  
  return 0
}

# Function to run tests
run_test() {
  local branch=$1
  
  echo "moving rocks database..."
  sudo mv /usr/local/stratus/data/rocksdb /usr/local/stratus/data/rocksdb_old-rocksdb

  echo "migrating rocks configuration..."
  sudo ./usr/local/stratus/bin/rocks_migrator --source /usr/local/stratus/data/rocksdb_old-rocksdb --destination /usr/local/stratus/data/rocksdb --batch-size 1000000

  echo "starting stratus service..."
  sudo systemctl start stratus

  echo "Running tests for branch: $branch"
  
  echo "Sending routine request to server..."
  curl -X POST "${SERVER_ENDPOINT}" \
       -H "Content-Type: application/json" \
       -d "{
           \"routine_type\": \"Cashier\",
           \"routine_config\": {
               \"cashier_target_account_strategy\": \"Random\",
               \"cashier_contract_address\": \"${CASHIER_CONTRACT}\",
               \"cashier_private_keys_file\": \"${PRIVATE_KEYS_FILE}\"
           },
           \"leader_http_address\": \"${LEADER_HTTP_ADDRESS}\",
           \"run_duration\": ${RUN_DURATION}
       }"
  
  echo "Routine request sent. Monitoring status..."
  
  # Monitor the status of the routine
  local is_running=true
  local has_error=false
  local error_message=""
  
  while [ "$is_running" = true ]; do
    echo "Checking routine status..."
    
    # Get the status from the endpoint
    local status_response=$(curl -s "${STATUS_ENDPOINT}")
    
    # Parse the JSON response using jq
    if ! command -v jq &> /dev/null; then
      echo "Error: jq is required for JSON parsing but it's not installed."
      return 1
    fi
    
    # Extract values from the JSON response
    is_running=$(echo "$status_response" | jq -r '.is_running')
    has_error=$(echo "$status_response" | jq -r '.error != null')
    
    if [ "$has_error" = "true" ]; then
      error_message=$(echo "$status_response" | jq -r '.error')
      echo "Error detected: $error_message"
      return 1
    fi
    
    if [ "$is_running" = "true" ]; then
      echo "Routine is still running. Waiting ${STATUS_CHECK_INTERVAL} seconds before checking again..."
      sleep $STATUS_CHECK_INTERVAL
    else
      echo "Routine has completed successfully."
      # Print the latest results
      echo "Results:"
      echo "$status_response" | jq '.latest_results'
    fi
  done
  
  return 0
}

# Main function to process a list of branches
process_branches() {
  local branches=("$@")
  local success_count=0
  local failed_count=0
  local failed_branches=()
  
  for branch in "${branches[@]}"; do
    echo ""
    echo "=========================================="
    echo "Processing branch: $branch"
    echo "=========================================="
    
    if check_workflow_run "$branch"; then
      echo "Found existing successful workflow run. Downloading artifacts..."
      if download_artifacts "$branch"; then
        echo "✅ Successfully processed branch: $branch"
        run_test "$branch"
        ((success_count++))
      else
        echo "❌ Failed to download artifacts for branch: $branch"
        ((failed_count++))
        failed_branches+=("$branch (artifact download failed)")
      fi
    else
      echo "No successful workflow run found. Triggering a new workflow run..."
      
      if [ -z "$GITHUB_TOKEN" ]; then
        echo "❌ Cannot trigger workflow without a GITHUB_TOKEN. Please set the GITHUB_TOKEN environment variable."
        ((failed_count++))
        failed_branches+=("$branch (no GITHUB_TOKEN)")
        continue
      fi
      
      if trigger_workflow_run "$branch"; then
        echo "Waiting for workflow run to complete..."
        
        if wait_for_workflow_run "$branch"; then
          echo "Workflow run completed successfully. Downloading artifacts..."
          if download_artifacts "$branch"; then
            echo "✅ Successfully processed branch: $branch"
            run_test "$branch"
            ((success_count++))
          else
            echo "❌ Failed to download artifacts for branch: $branch"
            ((failed_count++))
            failed_branches+=("$branch (artifact download failed)")
          fi
        else
          echo "❌ Workflow run failed or timed out for branch: $branch"
          ((failed_count++))
          failed_branches+=("$branch (workflow failed or timed out)")
        fi
      else
        echo "❌ Failed to trigger workflow run for branch: $branch"
        ((failed_count++))
        failed_branches+=("$branch (workflow trigger failed)")
      fi
    fi
    
    echo "-----------------------------------"
  done
  
  # Print summary
  echo ""
  echo "=========================================="
  echo "Summary"
  echo "=========================================="
  echo "Total branches processed: $((success_count + failed_count))"
  echo "Successful: $success_count"
  echo "Failed: $failed_count"
  
  if [ $failed_count -gt 0 ]; then
    echo ""
    echo "Failed branches:"
    for failed in "${failed_branches[@]}"; do
      echo "  - $failed"
    done
    return 1
  fi
  
  return 0
}

# Check if branches are provided as arguments
if [ $# -eq 0 ]; then
  echo "Usage: $0 branch1 [branch2 ...]"
  echo "Example: $0 main feature/new-feature"
  exit 1
fi

# Process the branches
if process_branches "$@"; then
  echo "All branches processed successfully!"
  exit 0
else
  echo "Some branches failed to process. Check the summary above for details."
  exit 1
fi