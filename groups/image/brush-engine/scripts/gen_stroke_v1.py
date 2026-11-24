"""Regenerate from the forge root:
python -I groups/image/brush-engine/scripts/gen_stroke_v1.py
"""

from pathlib import Path
import sys


def stroke_text():
    rows = ["schema=1", "seq,t_ns,window,device,kind,a,b,c,d,e"]
    for i in range(49):
        kind = "down" if i == 0 else "move"
        pressure = (512 * i + 24) // 48
        rows.append(
            f"{i},{i * 5000000},1,pen,{kind},"
            f"{16 + 2 * i}.000,32.000,{pressure},0,0"
        )
    rows.append("49,245000000,1,pen,up,112.000,32.000,0,0,0")
    return "\n".join(rows) + "\n"


def main():
    arguments = sys.argv[1:]
    if arguments not in ([], ["--check"]):
        print("usage: gen_stroke_v1.py [--check]")
        return 2
    path = Path(__file__).resolve().parent.parent / "tests/fixtures/stroke_v1.csv"
    expected = stroke_text()
    if arguments:
        try:
            with path.open("r", encoding="utf-8", newline="") as source:
                actual = source.read()
        except (OSError, UnicodeError) as error:
            print(f"gen_stroke_v1: FAIL differs ({error})")
            return 1
        if actual != expected:
            print("gen_stroke_v1: FAIL differs")
            return 1
        print("gen_stroke_v1: ok")
        return 0
    try:
        with path.open("w", encoding="utf-8", newline="\n") as output:
            output.write(expected)
    except (OSError, UnicodeError) as error:
        print(f"gen_stroke_v1: FAIL write ({error})")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
