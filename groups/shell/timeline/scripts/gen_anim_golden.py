"""Regenerate: python -I groups/shell/timeline/scripts/gen_anim_golden.py

Independent decimal polynomial reference, with no Rust invocation.
"""

from decimal import Decimal, localcontext
from pathlib import Path


def generate():
    rows = ["frame,position,rotation,scale"]
    with localcontext() as context:
        context.prec = 60
        for frame in range(150):
            t = Decimal(frame) / Decimal(150)
            position = Decimal(10) * t
            rotation = Decimal(0) if frame < 75 else Decimal(90)
            scale = Decimal(1) + Decimal(3) * t * t - Decimal(2) * t * t * t
            rows.append(
                f"{frame},{position:.15f},{rotation:.15f},{scale:.15f}"
            )
    return "\n".join(rows) + "\n"


if __name__ == "__main__":
    destination = Path(__file__).resolve().parents[1] / "tests/fixtures/anim_golden.csv"
    destination.write_bytes(generate().encode("ascii"))
