#!/usr/bin/env bash
# Runs a CI command again when it fails, so a runner's network blip (DNS,
# crates.io, the Rust toolchain server) does not fail the build. Use it only
# for steps that download and are safe to repeat:
#   bash .github/retry.sh cargo fetch --locked
# Tries 5 times, waiting 15, 30, 45 and 60 s between attempts.
set -uo pipefail

attempts=5
for attempt in $(seq 1 "$attempts"); do
  "$@" && exit 0
  status=$?
  if [ "$attempt" -eq "$attempts" ]; then
    echo "::error::'$*' failed $attempts times (last exit $status)"
    exit "$status"
  fi
  wait_s=$((attempt * 15))
  echo "::warning::'$*' failed (attempt $attempt of $attempts, exit $status); retrying in ${wait_s} s"
  sleep "$wait_s"
done
