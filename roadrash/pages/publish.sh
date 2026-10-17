#!/bin/bash
# Build the encrypted site on top of what's on GitHub and push it (no force).
set -e
cd ~/theseus-roadrash/deploy-gh
git -C site fetch -q origin main
node build.mjs | git -C site fast-import --quiet --force
git -C site push origin main
