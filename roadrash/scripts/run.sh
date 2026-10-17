#!/bin/bash
# Headless (no window) and silent by default; RR_WINDOW=1 shows the window.
# usage: run.sh <logfile> <seconds> [screenshot.png]  -- runs translated Road Rash from game/ROADRASH, audio muted
cd ~/theseus-roadrash/game/ROADRASH
THESEUS_HEADLESS=$([ -n "$RR_WINDOW" ] || echo 1) SDL_AUDIODRIVER=dummy RUST_BACKTRACE=1 ~/theseus-roadrash/theseus/target/release/roadrash > "$1" 2>&1 &
pid=$!; start=$(date +%s)
for i in $(seq 1 $2); do sleep 1; kill -0 $pid 2>/dev/null || break; done
if kill -0 $pid 2>/dev/null; then [ -n "$3" ] && screencapture -x "$3"; echo "still running after $2 s; killed"; kill $pid; fi
wait $pid; echo "exit=$? after $(( $(date +%s)-start )) s"
