#!/usr/bin/env bats

setup() {
    TMP="$(mktemp -d)"
}

@test "deploy succeeds with valid config" {
    run deploy --config ok.yaml
    assert_success
    [ "$status" -eq 0 ]
}

@test "deploy rejects a missing config" {
    run deploy --config missing.yaml
    assert_failure
}
