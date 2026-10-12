#!/bin/bash
set -e

cd "$(git rev-parse --show-toplevel)"
args=(
    #--trace
    --exe scratch/dos/sbaitso/SBTALKER.EXE
    --out out/sbaitso/sbtalker
    # TSR entry point, called by users of TSR.
    --entry-point 823:898
    # Driver entry point, returned by TSR.
    --entry-point 823:ae2
    # command dispatcher, subcommand indexes into jmp table
    --jump-table 823:d8e..823:da0
    # Random jump tables deep in the binary, uhoh.
    --jump-table d72:32c..d72:3ac
    --jump-table 935:3c40..935:3c58

    # Fake return address used after TSR loads.
    --extern 0:11=driver_loaded

    # BLASTER.DRV
    # --load 34c4:scratch/dos/sbaitso/BLASTER.DRV
    # API entry points
    # --entry-point 34c4:0000
    --extern 34c4:0000=audio_put_sample
    # --entry-point 34c4:0005
    --extern 34c4:0005=audio_init
    # --entry-point 34c4:000a
    --extern 34c4:000a=audio_flush
    # --entry-point 34c4:000f
    --extern 34c4:000f=audio_get_buffer
    # --entry-point 34c4:0014
    --extern 34c4:0014=audio_noop
    # IRQ handler
    # --entry-point 34c4:0e2c
)
cargo run -p tc -- "${args[@]}"
