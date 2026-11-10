#!/usr/bin/env bash
set -euo pipefail

PBRS_APT_TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$PBRS_APT_TEST_ROOT"' EXIT

run_case() {
  local label="$1" failures="$2" update_exit="$3" install_exit="$4"
  local expected_status="$5" expected_updates="$6" expected_installs="$7" expected_sleeps="$8"
  shift 8
  local PBRS_APT_TEST_DIR="$PBRS_APT_TEST_ROOT/$label"
  mkdir -p "$PBRS_APT_TEST_DIR"
  local PBRS_APT_TEST_FAILURES="$failures"
  local PBRS_APT_TEST_UPDATE_EXIT="$update_exit"
  local PBRS_APT_TEST_INSTALL_EXIT="$install_exit"
  local status updates=0 installs=0 sleeps=0 line

  set +e
  (
    # shellcheck source=scripts/ci-install-apt.sh
    source scripts/ci-install-apt.sh
    sudo() {
      printf '%s\n' "$*" >> "$PBRS_APT_TEST_DIR/calls"
      [[ "$1" == timeout && "$2" == --kill-after=5s ]] || return 98
      if [[ "${*: -1}" == update ]]; then
        [[ "$3" == 90s && "$*" == *"APT::Update::Error-Mode=any"* ]] || return 98
        [[ "$*" == *"Acquire::http::Timeout=20"* && "$*" == *"Acquire::https::Timeout=20"* ]] || return 98
        local n=0
        if [[ -f "$PBRS_APT_TEST_DIR/updates" ]]; then
          read -r n < "$PBRS_APT_TEST_DIR/updates"
        fi
        n=$((n + 1))
        printf '%s\n' "$n" > "$PBRS_APT_TEST_DIR/updates"
        if (( n <= PBRS_APT_TEST_FAILURES )); then
          return "$PBRS_APT_TEST_UPDATE_EXIT"
        fi
        return 0
      fi
      if [[ "$*" == *" install -y "* ]]; then
        [[ "$3" == 120s && "$*" == *"DPkg::Lock::Timeout=30"* ]] || return 98
        [[ "$*" == *" protobuf-compiler cmake g++" ]] || return 98
        printf '1\n' >> "$PBRS_APT_TEST_DIR/installs"
        return "$PBRS_APT_TEST_INSTALL_EXIT"
      fi
      return 98
    }
    sleep() {
      [[ "$1" == 3 ]] || return 98
      printf '1\n' >> "$PBRS_APT_TEST_DIR/sleeps"
    }
    ci_install_apt "$@"
  ) > "$PBRS_APT_TEST_DIR/output" 2>&1
  status=$?
  set -e

  if [[ -f "$PBRS_APT_TEST_DIR/updates" ]]; then
    read -r updates < "$PBRS_APT_TEST_DIR/updates"
  fi
  if [[ -f "$PBRS_APT_TEST_DIR/installs" ]]; then
    while IFS= read -r line; do installs=$((installs + 1)); done < "$PBRS_APT_TEST_DIR/installs"
  fi
  if [[ -f "$PBRS_APT_TEST_DIR/sleeps" ]]; then
    while IFS= read -r line; do sleeps=$((sleeps + 1)); done < "$PBRS_APT_TEST_DIR/sleeps"
  fi
  if [[ "$status" != "$expected_status" || "$updates" != "$expected_updates" ||
        "$installs" != "$expected_installs" || "$sleeps" != "$expected_sleeps" ]]; then
    cat "$PBRS_APT_TEST_DIR/output" >&2
    echo "$label: status=$status updates=$updates installs=$installs sleeps=$sleeps" >&2
    exit 1
  fi
  echo "PASS $label"
}

run_case immediate 0 100 0 0 1 1 0 protobuf-compiler cmake g++
run_case update_retry 1 100 0 0 2 1 1 protobuf-compiler cmake g++
run_case updates_fail 2 100 0 100 2 0 1 protobuf-compiler cmake g++
run_case update_timeout 2 124 0 124 2 0 1 protobuf-compiler cmake g++
run_case update_killed 2 137 0 137 2 0 1 protobuf-compiler cmake g++
run_case install_fails 0 100 101 101 1 1 0 protobuf-compiler cmake g++
run_case no_packages 0 100 0 2 0 0 0
