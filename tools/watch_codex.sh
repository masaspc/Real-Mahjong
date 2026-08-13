#!/usr/bin/env bash
# Codex のジョブを見張る。
#
# **状態の変化だけを見てはならない。**Codex は "running" のまま静かに
# 死ぬことがある。Wave 3c の第2ラウンドは 6 時間 43 分そのままだったが、
# 状態変化だけを見ていた監視は一度も鳴らなかった。ログが伸びなくなった
# ことを検出しないと、止まったことに気づけない。
#
# 使い方: tools/watch_codex.sh <job-id> [停滞とみなす秒数]

set -u
JOB="${1:?job-id を渡すこと}"
STALL="${2:-600}"
COMPANION="$HOME/.claude/plugins/cache/openai-codex/codex/1.0.6/scripts/codex-companion.mjs"
LOG_DIR="$HOME/.claude/plugins/data/codex-openai-codex/state"

find_log() {
  find "$LOG_DIR" -name "${JOB}.log" -type f 2>/dev/null | head -1
}

age_of() {
  local f="$1"
  local m
  m=$(stat -f %m "$f" 2>/dev/null || stat -c %Y "$f" 2>/dev/null) || return 1
  echo $(( $(date +%s) - m ))
}

prev_state=""
warned_stall=0

while true; do
  out=$(node "$COMPANION" status "$JOB" 2>&1 || true)
  state=$(printf '%s' "$out" | grep -oE '\| (running|completed|failed|cancelled|error) \|' | head -1 | tr -d '| ')
  [ -z "$state" ] && state="unknown"

  if [ "$state" != "$prev_state" ]; then
    echo "Codex $JOB: $state"
    prev_state="$state"
  fi

  case "$state" in
    completed|failed|cancelled|error)
      echo "Codex $JOB 終了: $state"
      exit 0
      ;;
  esac

  log=$(find_log)
  if [ -n "$log" ]; then
    age=$(age_of "$log" || echo 0)
    if [ "$age" -ge "$STALL" ]; then
      if [ "$warned_stall" -eq 0 ] || [ $(( age % 1800 )) -lt 60 ]; then
        echo "Codex $JOB が停滞: ログが ${age} 秒伸びていない（状態は $state のまま）。ハングの疑い"
        warned_stall=1
      fi
    elif [ "$warned_stall" -eq 1 ]; then
      echo "Codex $JOB が動き出した"
      warned_stall=0
    fi
  fi

  sleep 60
done
