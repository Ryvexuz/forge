"""Regenerate from the forge root:
python -I groups/shell/timeline/scripts/gen_curve_golden.py

Independent exact integer Bernstein/Hermite polynomials, not Rust evaluation.
Bezier x handles are 1/3 and 2/3, making x(t)=t exactly.
Output has 1001 samples for each mode and each endpoint pair.
"""

from pathlib import Path


def rows():
    yield "pair,mode,sample,value"
    scale = 1000
    denominator = scale**3
    for pair, (a, b) in enumerate(((0, 1), (-7, 4), (12, -3))):
        for mode in ("step", "linear", "bezier", "hermite"):
            for i in range(scale + 1):
                u = scale - i
                if mode == "step":
                    n = (b if i == scale else a) * denominator
                elif mode == "linear":
                    n = (a * u + b * i) * scale**2
                elif mode == "bezier":
                    n = a * u**3 + 3 * (a + 3) * u**2 * i
                    n += 3 * (b - 1) * u * i**2 + b * i**3
                else:
                    n = a * (2 * i**3 - 3 * scale * i**2 + denominator)
                    n += b * (-2 * i**3 + 3 * scale * i**2)
                    n += 9 * (i**3 - 2 * scale * i**2 + scale**2 * i)
                    n += 3 * (i**3 - scale * i**2)
                sign = "-" if n < 0 else ""
                whole, fraction = divmod(abs(n), denominator)
                yield f"{pair},{mode},{i},{sign}{whole}.{fraction:09d}"


if __name__ == "__main__":
    output = Path(__file__).resolve().parent.parent / "tests/fixtures/curve_golden.csv"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(rows()) + "\n", encoding="ascii")
