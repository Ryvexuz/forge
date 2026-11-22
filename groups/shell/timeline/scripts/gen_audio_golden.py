"""Independent reference: 256 mono samples at 48 kHz, little-endian f32.

Two tracks: (start, end, Hz, amplitude, fade-in, fade-out):
(0, 220, 1000, .5, 16, 32), (37, 256, 375, -.25, 23, 41).
Run from any directory; the output is relative to this script.
"""

import math
from pathlib import Path
import struct


def samples():
    for index in range(256):
        total = 0.0
        for start, end, hz, amplitude, fade_in, fade_out in (
            (0, 220, 1000, 0.5, 16, 32),
            (37, 256, 375, -0.25, 23, 41),
        ):
            if start <= index < end:
                local = index - start
                gain = min(1.0, local / fade_in, (end - 1 - index) / fade_out)
                total += math.sin(2 * math.pi * hz * local / 48000) * amplitude * gain
        yield total


if __name__ == "__main__":
    destination = Path(__file__).resolve().parent.parent / "tests/fixtures/audio_golden.bin"
    destination.write_bytes(b"".join(struct.pack("<f", value) for value in samples()))
