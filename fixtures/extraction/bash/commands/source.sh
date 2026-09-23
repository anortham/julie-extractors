#!/usr/bin/env bash
# Deploy helpers for the audit service.

# Shared logging helpers.
source ./lib/log.sh

# Log file for this run.
log_file=/tmp/deploy.log

# Remove the scratch directory.
cleanup() {
    rm -rf "$TMP"
}

render() {
    local name="${1:-world}"
    local greeting="${2}"
    echo "$greeting $name from $(basename "$0")"
}

# Register handlers and run the deploy.
deploy() {
    # Handlers run on exit and on interrupt.
    trap cleanup EXIT
    trap 'render "interrupted"; exit 130' INT
    local -r ATTEMPTS=3
    local result
    declare -A seen
    seen[start]=1
    while IFS= read -r line; do
        echo "$line"
    done < hosts.txt
    timeout 30 render
    sudo -E env MODE=prod render "$result"
    command -v jq
    getopts ":v" opt
    "$TOOL_DIR/bin/tool" --flag
    curl -X POST "https://api.example.com/v1/deploy" -H 'Content-Type: application/json'
    wget -qO- "https://example.com/health"
    psql "$DATABASE_URL" <<SQL
CREATE TABLE IF NOT EXISTS audit (id serial PRIMARY KEY);
SQL
    mysql -u root <<< "DROP DATABASE staging"
    complete -F render deploy
}

# Install root on the target host.
ROOT=/srv/app
export ROOT
export PATH="$ROOT/bin:$PATH" EDITOR=vim
declare -gx DEPLOY_ENV=prod
