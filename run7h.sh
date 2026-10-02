#!/bin/bash
# 7-hour live run on main, now carrying both the #34 silence-detection fix
# and the #17 share-reply pairing (merged 2026-10-01 as PR #35 and PR #36).
#
# Adapted from run12h.sh (same guards, same caffeinate discipline) for the
# primary checkout rather than the share-ids worktree, and for a 7h window
# rather than 12h.
set -euo pipefail
cd "$(dirname "$0")"

BIN=./target/release/minertim
LOG="$PWD/LIVE7H_RUN.log"
SECONDS_TO_RUN=25200          # 7h

# Refuse to start if this binary lacks either fix — the whole run turns on it.
strings "$BIN" | grep -c 'No data from pool for' >/dev/null || {
    echo "REFUSING: $BIN has no silence detection (#34); rebuild from main." >&2
    exit 1
}
strings "$BIN" | grep -c 'Share lost: rpc_id' >/dev/null || {
    echo "REFUSING: $BIN has no share-reply pairing (#17); rebuild from main." >&2
    exit 1
}

POOL=$(grep '^POOL='    mining.conf | cut -d= -f2)
WALLET=$(grep '^WALLET=' mining.conf | cut -d= -f2)
THREADS=$(grep '^THREADS=' mining.conf | cut -d= -f2)

: > "$LOG"
# perl alarm: the miner limits itself, so there is no external stopper to lose
# (an earlier run overran by 4h when a `sleep; kill` helper vanished).
nohup caffeinate -dimsu perl -e 'alarm shift; exec @ARGV' \
      "$SECONDS_TO_RUN" "$BIN" "$POOL" "$WALLET" "$THREADS" >> "$LOG" 2>&1 &
echo $! > /tmp/miner7h.pid
sleep 15

MPID=$(cat /tmp/miner7h.pid)
# caffeinate forks the assertion-holder and execs the utility, so the holder is
# a CHILD of the miner. Record ITS pid — checking for "some caffeinate" is what
# masked a failure before.
CAFF=$(ps -Ao pid,ppid,comm | awk -v p="$MPID" '$2==p && $3 ~ /caffeinate/ {print $1}' | head -1)
[ -n "$CAFF" ] || { echo "REFUSING: no caffeinate child of $MPID; run is unprotected." >&2; kill "$MPID"; exit 1; }
echo "$CAFF" > /tmp/caffeinate7h.pid
pmset -g assertions | grep -c "pid $CAFF(" >/dev/null || {
    echo "REFUSING: caffeinate $CAFF holds no assertion." >&2; kill "$MPID"; exit 1; }

echo "started  miner=$MPID  caffeinate=$CAFF  pool=$POOL threads=$THREADS"
echo "log      $LOG"
echo "ends     $(date -v +7H '+%Y-%m-%d %H:%M %Z')"
