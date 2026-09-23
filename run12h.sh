#!/bin/bash
# 12-hour live verification of the #34 silence-detection fix.
#
# Runs the binary built from fix/stale-connection-detect — NOT main, which has
# no silence detection. The point of the run is to see whether a real silent
# pool is now caught, so running main's binary would prove nothing.
set -euo pipefail
cd "$(dirname "$0")"

BIN=./target/release/minertim
LOG="$PWD/LIVE12H_FIX.log"
SECONDS_TO_RUN=43200          # 12h

# Refuse to start if this binary lacks the fix — the whole run turns on it.
strings "$BIN" | grep -q 'No data from pool for' || {
    echo "REFUSING: $BIN has no silence detection; rebuild from the fix branch." >&2
    exit 1
}
# And the #17 share-reply pairing, since tonight's run is meant to carry both.
strings "$BIN" | grep -q 'Share lost: rpc_id' || {
    echo "REFUSING: $BIN has no share-reply pairing (#17); build from fix/share-response-ids." >&2
    exit 1
}

POOL=$(grep '^POOL='    ../../../mining.conf | cut -d= -f2)
WALLET=$(grep '^WALLET=' ../../../mining.conf | cut -d= -f2)
THREADS=$(grep '^THREADS=' ../../../mining.conf | cut -d= -f2)

: > "$LOG"
# perl alarm: the miner limits itself, so there is no external stopper to lose
# (the last run overran by 4h when a `sleep; kill` helper vanished). Verified
# that alarm survives exec.
nohup caffeinate -dimsu perl -e 'alarm shift; exec @ARGV' \
      "$SECONDS_TO_RUN" "$BIN" "$POOL" "$WALLET" "$THREADS" >> "$LOG" 2>&1 &
echo $! > /tmp/miner.pid
sleep 15

MPID=$(cat /tmp/miner.pid)
# caffeinate forks the assertion-holder and execs the utility, so the holder is
# a CHILD of the miner. Record ITS pid — checking for "some caffeinate" is what
# masked a failure before.
CAFF=$(ps -Ao pid,ppid,comm | awk -v p="$MPID" '$2==p && $3 ~ /caffeinate/ {print $1}' | head -1)
[ -n "$CAFF" ] || { echo "REFUSING: no caffeinate child of $MPID; run is unprotected." >&2; kill "$MPID"; exit 1; }
echo "$CAFF" > /tmp/caffeinate.pid
pmset -g assertions | grep -q "pid $CAFF(" || {
    echo "REFUSING: caffeinate $CAFF holds no assertion." >&2; kill "$MPID"; exit 1; }

echo "started  miner=$MPID  caffeinate=$CAFF  pool=$POOL threads=$THREADS"
echo "log      $LOG"
echo "ends     $(date -v +12H '+%Y-%m-%d %H:%M %Z')"
