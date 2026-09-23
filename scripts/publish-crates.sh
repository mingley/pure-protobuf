#!/usr/bin/env bash
# Publish workspace crates to crates.io in dependency order.
#
# Names and versions come from each crate's [package] table, not literals.
# If a version is already on the index, skip the real upload (idempotent retry).
# DRY_RUN=1 still packs so a no-publish rehearsal names exact .crate artifacts.
#
# Usage:
#   DRY_RUN=1 ./scripts/publish-crates.sh
#   CARGO_REGISTRY_TOKEN=... ./scripts/publish-crates.sh
#   RELEASE_TAG=v0.1.0 ./scripts/publish-crates.sh   # tag must match a crate version
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DRY_RUN="${DRY_RUN:-0}"
case "$DRY_RUN" in
  0|1) ;;
  *)
    echo "::error::DRY_RUN must be 0 (publish) or 1 (offline package only)" >&2
    exit 2
    ;;
esac
UA="pure-protobuf-release/1"

pkg_field() {
  local manifest="$1" field="$2"
  awk -v field="$field" '
    $0 == "[package]" { inpkg=1; next }
    /^\[/ { inpkg=0 }
    inpkg && $1 == field && $2 == "=" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$manifest"
}

# Ordered: core first, then adapters (both depend only on pbrs).
MANIFESTS=(
  "${ROOT}/Cargo.toml"
  "${ROOT}/protobuf-tonic/Cargo.toml"
  "${ROOT}/pbrs-grpc/Cargo.toml"
)

declare -a NAMES=()
declare -a VERS=()
for manifest in "${MANIFESTS[@]}"; do
  name="$(pkg_field "$manifest" name)"
  ver="$(pkg_field "$manifest" version)"
  if [[ -z "$name" || -z "$ver" ]]; then
    echo "::error::failed to parse name/version from ${manifest}" >&2
    exit 1
  fi
  echo "Manifest ${manifest}: ${name} ${ver}"
  NAMES+=("$name")
  VERS+=("$ver")
done

if [[ -n "${RELEASE_TAG:-}" ]]; then
  tag="${RELEASE_TAG#v}"
  match=0
  for ver in "${VERS[@]}"; do
    if [[ "$ver" == "$tag" ]]; then
      match=1
      break
    fi
  done
  if [[ "$match" -ne 1 ]]; then
    echo "::error::tag '${RELEASE_TAG}' does not match any crate version: ${NAMES[*]} ${VERS[*]}" >&2
    echo "Adapters are not lockstep with pbrs; tag v<version> of a crate you are actually publishing." >&2
    exit 1
  fi
  echo "Tag ${RELEASE_TAG} matches a manifest version"
fi

already_on_index() {
  local name="$1" ver="$2" code
  if ! code="$(curl --connect-timeout 5 --max-time 20 -sS -o /dev/null -w '%{http_code}' -A "$UA" \
    "https://crates.io/api/v1/crates/${name}/${ver}")"; then
    echo "::error::crates.io API request failed for ${name} ${ver}; refusing to publish without an index check" >&2
    return 2
  fi
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    *)
      echo "::error::crates.io API returned HTTP ${code} for ${name} ${ver}; refusing to publish" >&2
      return 2
      ;;
  esac
}

wait_for_index() {
  local name="$1" ver="$2" i status
  for i in $(seq 1 30); do
    if already_on_index "$name" "$ver"; then
      echo "${name} ${ver} is live on crates.io"
      return 0
    else
      status=$?
      if [[ "$status" -ne 1 ]]; then
        return "$status"
      fi
    fi
    echo "Waiting for crates.io index ${name} ${ver} (${i}/30)..."
    sleep 10
  done
  echo "::error::crates.io API does not yet show ${name} ${ver}" >&2
  return 1
}

for i in "${!NAMES[@]}"; do
  name="${NAMES[$i]}"
  ver="${VERS[$i]}"
  crate_file="target/package/${name}-${ver}.crate"

  if [[ "$DRY_RUN" == "1" ]]; then
    # Always pack, even when this version is already on the index.
    # Adapters resolve the matching local core while packing offline, even
    # when a fresh registry index has not seen this version of pbrs yet.
    # The command-line patch is not written into the .crate manifest.
    package_args=(package -p "$name" --no-verify --offline)
    if [[ "$i" -gt 0 ]]; then
      package_args+=(--config "patch.crates-io.${NAMES[0]}.path=\"$ROOT\"")
    fi
    cargo "${package_args[@]}"
    echo "dry-run packed ${crate_file}"
    continue
  fi

  if already_on_index "$name" "$ver"; then
    echo "${name} ${ver} already on crates.io — skipping (idempotent)"
    continue
  else
    status=$?
    if [[ "$status" -ne 1 ]]; then
      exit "$status"
    fi
  fi

  if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
    echo "::error::CARGO_REGISTRY_TOKEN is not set (map from secret CRATES_IO_TOKEN)" >&2
    exit 1
  fi

  cargo publish -p "$name"
  wait_for_index "$name" "$ver"
done
