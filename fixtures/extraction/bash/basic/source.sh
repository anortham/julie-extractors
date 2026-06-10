#!/usr/bin/env bash

helper() {
    local value="$1"
    echo "$((value + 1))"
}

run_worker() {
    helper "$1"
}

evaluate() {
    local count=$1
    local enabled=$2
    local total=0
    if [ "$enabled" = "true" ]; then
        for i in $(seq 1 "$count"); do
            total=$((total + i))
        done
    elif [ "$count" -gt 0 ]; then
        total=1
    fi
    echo "$total"
}

export APP_ENV="production"
