#!/bin/bash
#
# Clone Git repositories containing Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Clone a project to the projects directory.
clone() {
    repo=brlc-monorepo
    target=../brlc-monorepo

    if [ -d "$target" ]; then
        log "Updating: $repo"
        git -C "$target" pull
    else
        log "Cloning: $repo"
        if [ -n "${BRLC_MONOREPO_TOKEN:-}" ]; then
            # Authenticate through an ephemeral HTTP header so the PAT is not
            # persisted in the clone's origin URL or .git/config.
            auth=$(printf 'x-access-token:%s' "$BRLC_MONOREPO_TOKEN" | base64 | tr -d '\n')
            if ! GIT_CONFIG_COUNT=1 \
                GIT_CONFIG_KEY_0=http.https://github.com/.extraheader \
                GIT_CONFIG_VALUE_0="AUTHORIZATION: basic $auth" \
                git clone "https://github.com/cloudwalk/${repo}.git" -b main "$target"; then
                log "Clone failed. Removing folder and exiting."
                rm -rf "$target"
                return 1
            fi
        else
            if ! git clone "https://github.com/cloudwalk/${repo}.git" -b main "$target"; then
                log "Clone failed. Removing folder and exiting."
                rm -rf "$target"
                return 1
            fi
        fi
    fi

    log "Installing dependencies: $repo"
    corepack enable
    # Run pnpm install from inside the cloned repo so corepack picks up the
    # correct pnpm version from its package.json (packageManager field).
    if ! (cd "$target" && corepack pnpm install); then
        log "Dependencies install failed. Removing folder and exiting."
        rm -rf "$target"
        return 1
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

log "Cloning or updating brlc-monorepo"

clone
