#!/bin/bash
set -euo pipefail

source "$(dirname "$0")/base-test.sh"
source "$DIR"/common.sh
source <(kubectl completion bash)

readonly MAX_RETRIES=3
declare -a HOSTS=(web1 web2)
declare -A ROUTES=([home]=/ [about]=/about)

count_items() {
    local total=0
    local limit=10
    (( total += limit ))
    total=$(( total * 2 + limit ))
    for (( n = 0; n < limit; n++ )); do
        echo "${HOSTS[n]}" "${ROUTES[home]}"
    done
    unset total
}

count_items
