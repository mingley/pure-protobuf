#!/usr/bin/env bash
# Bounded Ubuntu package setup for hosted CI. macOS uses Homebrew separately.

ci_install_apt() {
  if (( $# == 0 )); then
    echo "usage: ci-install-apt.sh PACKAGE..." >&2
    return 2
  fi

  local attempt status
  for attempt in 1 2; do
    if sudo timeout --kill-after=5s 90s apt-get \
      -o Acquire::Retries=1 \
      -o Acquire::http::Timeout=20 \
      -o Acquire::https::Timeout=20 \
      -o APT::Update::Error-Mode=any update; then
      break
    else
      status=$?
      echo "apt index update attempt $attempt failed (exit $status)" >&2
      if (( attempt == 2 )); then
        return "$status"
      fi
      sleep 3
    fi
  done

  # Do not retry an interrupted package installation: dpkg may need repair.
  sudo timeout --kill-after=5s 120s apt-get \
    -o DPkg::Lock::Timeout=30 \
    -o Acquire::Retries=1 \
    -o Acquire::http::Timeout=20 \
    -o Acquire::https::Timeout=20 install -y "$@"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  ci_install_apt "$@"
fi
