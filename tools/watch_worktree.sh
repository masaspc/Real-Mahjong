#!/bin/bash
# Orca の worktree で動くエージェントの状態変化を1行ずつ出す。
#
# Orca のエージェントはこちらへ通知を送らない。完了も、行き詰まりも、
# 見に行かなければ分からない。コミット数・作業ツリーの汚れ・worktree の
# コメントの3つを見て、変わったときだけ出力する。
#
# 使い方: tools/watch_worktree.sh <worktree名> <worktreeのパス>
set -u
NAME="$1"
DIR="$2"

snapshot() {
  local commits dirty comment
  commits=$(git -C "$DIR" rev-list --count HEAD 2>/dev/null || echo "?")
  dirty=$(git -C "$DIR" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
  comment=$(orca worktree ps --json 2>/dev/null | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
except Exception:
    print('')
    raise SystemExit
for w in d.get('result', {}).get('worktrees', []):
    if str(w.get('displayName') or w.get('name') or '') == '$NAME':
        print((w.get('comment') or '').replace(chr(10), ' '))
        break
" 2>/dev/null)
  echo "${commits}|${dirty}|${comment}"
}

previous=$(snapshot)
echo "[$NAME] 監視開始: ${previous}"
while true; do
  sleep 45
  current=$(snapshot)
  if [ "$current" != "$previous" ]; then
    IFS='|' read -r commits dirty comment <<< "$current"
    echo "[$NAME] コミット${commits} 未コミット${dirty}件 / ${comment}"
    previous="$current"
  fi
done
