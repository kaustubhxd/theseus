#!/bin/bash
# Play Road Rash (Theseus WebAssembly port) in your browser, locally.
#   ~/theseus-roadrash/play.sh          -> http://127.0.0.1:8880/
# Sound is off unless the URL has ?sound=1 (e.g. http://127.0.0.1:8880/?sound=1).
# Saves and settings are kept in the browser (IndexedDB) across reloads.
# Game data is served from ~/theseus-roadrash/game/ROADRASH (never uploaded).
set -e
cd ~/theseus-roadrash/theseus/web
export NODE_ENV=production VITE_CONFIG_NATIVE_IGNORE_WARNING=true
./node_modules/.bin/vite build --logLevel warn
echo "Road Rash: open http://127.0.0.1:8880/  (add ?sound=1 for sound)"
exec ./node_modules/.bin/vite preview --port 8880 --strictPort --host 127.0.0.1
