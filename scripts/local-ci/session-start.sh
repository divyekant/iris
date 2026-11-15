#!/bin/sh
# SessionStart hook (.claude/settings.json). In a Claude cloud session, it turns on the
# pre-push hook and starts the setup of every area in the background, so the first
# local CI run is fast. A run that starts before the warm-up ends waits for it.
# It never fails the session start.
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || [ -n "${LOCAL_CI_WARMUP:-}" ] || exit 0
cd "$(dirname "$0")/../.." || exit 0
[ -f starter.json ] || exit 0
git config core.hooksPath >/dev/null || git config core.hooksPath scripts/git-hooks
mkdir -p .local-ci && { [ -f .local-ci/.gitignore ] || echo '*' > .local-ci/.gitignore; }
nohup python3 scripts/local-ci/local_ci.py --prepare >.local-ci/prepare.log 2>&1 </dev/null &
exit 0
