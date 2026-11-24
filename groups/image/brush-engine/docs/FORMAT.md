# Provisional event log v1 and stroke replay

This is a **provisional** file contract until node 1.1 W3 lands. W3's fixture
then replaces `event_log_v1.csv` and reconciles these pen columns in a deliberate,
reviewed step. This copy is a file contract, not a dependency on window-input.

## Lines and columns

The UTF-8 text is nonempty, ends in LF (`\n`), and has no CR (`\r`) or empty
line. A final LF terminates the last line; it does not add an empty line.
Line 1 is `schema=<n>`, where `n` consists of decimal digits fitting `u64`.
Only version 1 is supported. Line 2 is exactly:

```text
seq,t_ns,window,device,kind,a,b,c,d,e
```

Those two lines alone form a valid, empty log. Every subsequent line has exactly
ten comma-separated fields, without quoting, escaping, comments or whitespace
trimming. Line numbers in errors and returned pen events are one-based.

| Column | Meaning on every row |
| --- | --- |
| `seq` | Decimal `u64`, starting at 0 and increasing by exactly 1 across all devices. |
| `t_ns` | Decimal `u64` nanoseconds since the first event (the producer's QueryPerformanceCounter clock); nondecreasing. Replay does not use a clock. |
| `window` | Decimal `u32` window identifier; replay uses one layer, not separate layers per window. |
| `device` | Nonempty token containing only `a-z`, `0-9`, `_`. Only `pen` is retained. |
| `kind` | Same token rule; pen kinds are exactly `down`, `move`, `up`. |
| `a` | Pen x coordinate; otherwise opaque device payload token. |
| `b` | Pen y coordinate; otherwise opaque device payload token. |
| `c` | Pen raw pressure; otherwise opaque device payload token. |
| `d` | Pen x tilt; otherwise opaque device payload token. |
| `e` | Pen y tilt; otherwise opaque device payload token. |

`seq`, `t_ns`, and `window` contain only ASCII decimal digits, with no sign and
no leading zero unless the entire value is `0`. Overflow is an error.
The reader does not require the first timestamp to be zero.

### Provisional pen fields

- `a`, `b`: layer pixels as an optional `-`, one or more ASCII digits, `.`,
  exactly three ASCII digits. Examples: `16.000`, `-0.125`. Leading zeros and
  negative zero are allowed. After syntax validation they are parsed as `f64`;
  nonfinite results are rejected. No plus sign, exponent, NaN, or infinity.
- `c`: ASCII digits parsed as `u32`, then restricted to `0..=1024`; leading
  zeros are permitted. Pressure is raw pressure divided by 1024 as `f32`.
- `d`, `e`: optional `-` followed by ASCII digits, parsed as `i32`; leading
  zeros and negative zero are permitted. No plus sign or decimal point.
  `TiltEllipse::from_degrees(d as f32, e as f32, 0.0)` must accept the vector:
  its magnitude must not exceed 90 degrees. Tilt is validated but not drawn.

Coordinate syntax has no arbitrary length cap. Replay's stabilizer accepts only
finite coordinates with magnitude at most its published `MAX_COORDINATE`
(`f32::MAX` represented as `f64`), leaving headroom for its regression.
Pressure response size and opacity remain in B3's bounded design range.

### Other devices

All five payload fields on a non-pen row must be nonempty tokens containing only
ASCII `A-Z`, `a-z`, `0-9`, `.`, `_`, `-`. They are not parsed as numbers.
Mouse, key, wheel, window, and unknown device tokens follow this same rule;
the provisional format assigns no semantic meaning to their payload positions.
They are validated, including sequence and timestamp, before being discarded.
No non-pen row changes the layer.

## Log errors and precedence

Empty input is checked first, then the final LF, before any line is inspected.
Thereafter the first bad line wins. Its line structure is checked before its
schema/header/fields. Field count precedes field contents, then fields are
checked left to right. Tilt vector validation follows parsing both components.

| `LogError` | Condition |
| --- | --- |
| `Empty` | No text. |
| `MissingFinalNewline` | Nonempty text does not end in LF. |
| `BadLine { line }` | The line is empty or contains CR. |
| `MissingSchema` | Line 1 is not `schema=` followed by decimal digits fitting `u64`. |
| `UnknownVersion { line, version }` | Parsed schema version is not 1 (line 1). |
| `BadHeader { line }` | Line 2 is missing or not the exact header (line 2). |
| `WrongFieldCount { line }` | A data row has other than ten comma-separated fields. |
| `BadNumber { line }` | Numeric syntax, integer range, or finite coordinate parsing fails. |
| `BadSequence { line }` | Sequence does not start at zero or is not the preceding sequence plus one, without wrapping. |
| `TimeBackwards { line }` | Timestamp is smaller than the preceding row's timestamp. |
| `BadToken { line }` | Device, kind or a non-pen payload violates its token rule. |
| `UnknownPenKind { line }` | A syntactically valid pen kind is not down/move/up. |
| `PressureOutOfRange { line }` | Raw pressure fits `u32` but exceeds 1024. |
| `BadTilt { line }` | Parsed tilt components fail the tilt-vector validator. |

`Display` includes the error variant and its fields, including the line when
present. No malformed input is silently converted to zero or clamped.

## Fixed stroke fixture

Regenerate from the forge root with
`python -I groups/image/brush-engine/scripts/gen_stroke_v1.py`.
The standard-library-only script resolves the output relative to its own file,
not the working directory, and writes UTF-8 with LF newlines.
`--check` writes nothing and compares the exact text, including newlines:
`gen_stroke_v1: ok` (exit 0), or `gen_stroke_v1: FAIL differs` (exit 1).
Any other arguments give usage and exit 2. Read/write failures also fail loudly.

After the schema and header, for each integer `i` from 0 through 48:

- `seq = i`, `t_ns = i * 5000000`, `window = 1`, `device = pen`.
- `kind = down` at zero, otherwise `move`.
- `a` is `f"{16 + 2*i}.000"`, `b` is `32.000`.
- `c = (512*i + 24) // 48`, computed with integer arithmetic.
- `d = e = 0`.

The last row is
`49,245000000,1,pen,up,112.000,32.000,0,0,0`.
The mixed-device fixture exercises two window identifiers and pressures
0, 512 and 1024; it is hand-specified, not a capture from hardware.

## Replay and flattening

`ReplaySettings` supplies width, height, RGB ink and hardness. Zero dimensions,
overflow of `width * height * 4`, an unrepresentable/unallocatable buffer, or
hardness rejected by `RoundDab::new` give `BadSettings` before any event.
Replay starts with transparent RGBA8 and one new B4 stabilizer.

Down opens a stroke (`DoubleDown { line }` if already open). Down and move sample
B3 at raw pressure / 1024, push the coordinate through B4, and stamp B2's round
dab at the filtered point. Its diameter is the response size, pressure is 1,
hardness is the setting, opacity is the response opacity, and RGB is the setting.
Move without an open stroke gives `NoPenDown { line }`.

Up also requires an open stroke. It calls `pen_up`, stamps the returned endpoint
if any using the last down/move pressure, then closes the stroke. The up row's
own pressure and coordinates were validated by the parser but do not drive the
flush. The stabilizer resets at pen-up. Windows do not partition strokes.
An open stroke at EOF gives `Unterminated` at the last pen row's line.
Pressure, stabilizer and dab failures retain the originating error and row line
in `ReplayError`. No partial `Replay` is returned on failure.
Every down, move and up counts as one frame. No clock, randomness or threads
participate. Publicly constructed events should satisfy the parser's contract.

`flatten_over_white` consumes a valid replay buffer (including the
`width * height * 4` invariant for manually constructed replays). Each straight
RGB channel `c` and alpha `a` is composited over opaque white using `u32`:

```text
(c*a + 255*(255 - a) + 127) / 255
```

Zero-alpha pixels become white. The returned PPM image retains the dimensions.
`ppm::write` encodes P6; the example writes into memory, not a filesystem path.

## Golden policy and example

The intended golden settings are `STROKE_V1_SETTINGS`: 160 by 64 pixels, black
ink, hardness 0.5. Only the ignored test `write_stroke_golden` writes
`tests/golden/stroke_v1.ppm`, under `CARGO_MANIFEST_DIR`. Generating and visually
reviewing the golden is a deliberate maintainer step, not a normal test action.
The nonignored tests check bounds, ink, the pressure ramp and symmetry
independently of the writer as well as byte-for-byte replay equality.
A change of f64 rounding requires deliberate golden regeneration with the
reason recorded alongside the review. Never silently replace exact bytes with
a tolerance.

The example validates the mixed-device log, then replays the generated stroke.
No flags prints `stroke_replay: ok frames=<n>` only on exact golden equality.
Exactly `--corrupt-dab` increments the first RGB ink channel with saturation for
every dab; a mismatch prints `stroke_replay: FAIL golden differing_bytes=<n>`
and exits 1. Other arguments are refused with exit 2. Parse/render failures
print the relevant `FAIL eventlog` or `FAIL replay` error and exit 1.
Refused arguments escape control characters so they cannot inject stdout lines.

## Orchestrator verification

Run these exact done-check commands from the Broadsword root, retaining output.
Neither an ungenerated fixture nor an ungenerated golden may be replaced with a
placeholder: both are compile-time inputs. The initial missing-golden bootstrap
must be resolved before the ignored writer can compile; this is not a passing
verification state. Only the named writer should produce the golden.

1. `build/bin/broadsword.exe test forge --task t-20261124-104341 > scratch/b6-test.txt 2>&1`
2. `build/bin/broadsword.exe check forge --task t-20261124-104341 > scratch/b6-check.txt 2>&1`
3. `build/bin/broadsword.exe run forge --task t-20261124-104341 -- cargo test --offline -p brush-engine --test replay -- --ignored write_stroke_golden > scratch/b6-golden.txt 2>&1`
4. `build/bin/broadsword.exe run forge --task t-20261124-104341 -- python -I groups/image/brush-engine/scripts/gen_stroke_v1.py --check > scratch/b6-gen.txt 2>&1`
5. `build/bin/broadsword.exe run forge --task t-20261124-104341 -- cargo run --offline --release -p brush-engine --example stroke_replay > scratch/b6-d7.txt 2>&1`
6. `build/bin/broadsword.exe run forge --task t-20261124-104341 -- cargo run --offline --release -p brush-engine --example stroke_replay -- --corrupt-dab > scratch/b6-d8.txt 2>&1`
7. `build/bin/broadsword.exe run forge --task t-20261124-104341 -- cargo run --offline --release -p brush-engine --example stroke_replay -- --other > scratch/b6-refuse.txt 2>&1`
8. `git -C projects/forge diff --stat main` and `git -C projects/forge check-attr -a groups/image/brush-engine/tests/golden/stroke_v1.ppm`
9. Apply each of the brief's seven fail-first mutations separately; for the test
   runs use `build/bin/broadsword.exe test forge --task t-20261124-104341 > scratch/mut.txt 2>&1`.
   Observe the specified behavioral failure, restore, and rerun unmutated.
   Mutation 4 also uses command 4; mutation 6 uses command 3 and compares
   `git hash-object` before and after restoration. None is a compile-only proof.

The orchestrator must additionally generate the stroke fixture with the command
in the Fixed stroke fixture section, review the golden visually, record its size and hash,
compare suite counts against the pre-task baseline, and run the mechanical
checks (including rustfmt check). No execution results are asserted here.
