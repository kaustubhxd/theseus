#!/usr/bin/env python3
"""Road Rash (Win95) .BOB -> PNG. A .BOB is raw 8-bit indexed pixels (no
header); colours come from DATA/PALETTE.RAW (256 x RGBX). The width isn't
stored, so it's chosen from the file size (worked out by viewing):
see WIDTH and guess_width."""
import sys, os
from PIL import Image
# Checked by eye: horizons 720x320, full screens 640x480, dashboards 640,
# clouds 512/256. Anything else: the width whose rows are most alike.
WIDTH = {230400: 720, 307200: 640, 131072: 512, 32768: 256, 76160: 640, 77440: 640}

def guess_width(d):
    def score(w):
        h = len(d) // w
        return sum(1 for y in range(1, h) for i in range(w) if d[(y-1)*w+i] != d[y*w+i]) / len(d)
    ws = [w for w in range(16, 1300) if len(d) % w == 0 and len(d) // w >= 4]
    return min(ws, key=score) if ws else None
def convert(src, dst, pal):
    d = open(src, 'rb').read()
    w = WIDTH.get(len(d)) or guess_width(d)
    if not w:
        print('skip (unknown size)', src, len(d)); return False
    im = Image.frombytes('P', (w, len(d) // w), d)
    im.putpalette(b''.join(pal[i*4:i*4+3] for i in range(256)))
    im.save(dst); return True
if __name__ == '__main__':
    game, out = sys.argv[1], sys.argv[2]
    pal = open(os.path.join(game, 'DATA', 'PALETTE.RAW'), 'rb').read()
    n = 0
    for root, _, files in os.walk(os.path.join(game, 'IMAGES')):
        for f in files:
            if f.upper().endswith('.BOB'):
                rel = os.path.relpath(os.path.join(root, f), os.path.join(game, 'IMAGES'))
                dst = os.path.join(out, os.path.splitext(rel)[0] + '.png')
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                n += convert(os.path.join(root, f), dst, pal)
    print('converted', n)
