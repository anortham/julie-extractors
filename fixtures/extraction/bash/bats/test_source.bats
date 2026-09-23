#!/usr/bin/env bats

load 'test_helper/bats-support/load'
bats_load_library bats-file

setup_file() {
    export SHARED=1
}

teardown_file() {
    rm -rf "$SHARED_DIR"
}

setup() {
    TMP="$(mktemp -d)"
}

@test "deploy succeeds with valid config" {
    run deploy --config ok.yaml
    assert_success
    [ "$status" -eq 0 ]
}

@test "deploy rejects a missing config" {
    load helpers
    run sudo deploy --config missing.yaml
    assert_failure
}
