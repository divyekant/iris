#!/bin/sh
# Local CI. Commit your change, run this command, then open the pull request.
# It tests the areas that the branch changes (starter.json) and pushes a proof commit.
set -eu
exec python3 "$(dirname "$0")/local_ci.py" "$@"
