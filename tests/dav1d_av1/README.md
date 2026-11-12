# dav1d AV1 fixture

`lossless_64x48.ivf` is synthetic: 12 pictures, 64x48, 8-bit 4:2:0, encoded losslessly with libaom. No game data is
in it. The pictures are a formula pattern (luma `x*3 + y*2 + f*5`, U `64 + x*2 + f*3`, V `192 - y*3 + f*2`, all mod
256), so `tests/dav1d_av1_test.c` can compare every decoded byte. It covers the pictures dav1d holds back until the end
of the stream, and a second pass after a flush.

Regenerate with an FFmpeg that has libaom-av1:

```sh
python3 - <<'PY'
from pathlib import Path
W, H, N = 64, 48, 12
out = bytearray()
for f in range(N):
    out += bytes((x * 3 + y * 2 + f * 5) & 0xff for y in range(H) for x in range(W))
    out += bytes((64 + x * 2 + f * 3) & 0xff for y in range(H // 2) for x in range(W // 2))
    out += bytes((192 - y * 3 + f * 2) & 0xff for y in range(H // 2) for x in range(W // 2))
Path("src.yuv").write_bytes(out)
PY
ffmpeg -hide_banner -loglevel error -y -f rawvideo -pix_fmt yuv420p -s 64x48 -r 30 -i src.yuv \
  -c:v libaom-av1 -aom-params lossless=1 -cpu-used 8 -g 6 -f ivf lossless_64x48.ivf
```
