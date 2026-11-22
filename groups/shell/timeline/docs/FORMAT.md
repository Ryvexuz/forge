# Timeline file format v1

UTF-8 text, tab-separated columns, LF-terminated records (including the last).
There are no comments, escaping, blank lines or implicit records. Record order
is significant: references must follow their definitions. Canonical writing
preserves track, clip, channel and key order; clips precede channels per track.

Records (column names below are not literal values):

- `timeline VERSION`: header, VERSION is 1.
- `track ID KIND NAME`: unique unsigned 64-bit ID, nonempty kind and name.
  The built-in kinds are audio, video and anim; custom kind strings are retained.
- `clip TRACK START LENGTH SOURCE OFFSET NAME`: signed 64-bit tick positions,
  nonnegative signed 64-bit length, source path and nonempty name.
- `channel TRACK TARGET`: appends a channel; its index is zero-based within
  its track. TARGET is a nonempty path.
- `key TRACK CHANNEL TIME VALUE INTERP IN_TIME IN_VALUE OUT_TIME OUT_VALUE`:
  signed 64-bit tick time, finite f64 value; interpolation is step, linear,
  bezier or hermite. The four handle offsets are all finite f64 values or
  all `-` for absent handles. Offsets in time are measured in ticks.

Names and kinds cannot contain control characters. Paths additionally reject
backslashes and colons (including Windows drive prefixes). Text is not escaped.
Floats use Rust's shortest round-trippable decimal representation, not a fixed
precision. No floating-point arithmetic is performed by persistence.
`write` requires valid in-memory data; plain public data structs do not enforce
these invariants. `parse` validates the entire file before returning any state.

`migrate(version, text)` dispatches supported historical schemas to the current
state. Currently only v1 exists; the supplied version and header must both be 1.
The schema-change test defines a v2 wrapper adding `muted`, default false,
without pretending that a production v2 schema exists.

## Named errors

- MissingHeader: absent or unrecognized first record.
- UnknownVersion: unsupported numeric header or migration version.
- WrongColumnCount: record has the wrong number of tab-separated columns.
- InvalidNumber: malformed integer or float, an overflowing integer, or negative length. A float that overflows to infinity (such as 1e999) parses and is NonFinite instead.
- NonFinite: a parsed scalar is NaN or infinite.
- InvalidText: empty/control-bearing text or unknown interpolation.
- InvalidPath: a path contains a backslash or colon.
- MissingTrack: a referenced track has not been defined.
- MissingChannel: a key references an undefined channel index.
- DuplicateTrack: a track ID is repeated.
- UnknownRecord: an unrecognized record tag (including a blank record).
- TruncatedRecord: no final LF after a recognized header.

Errors implement Display and std::error::Error. No partial timeline is returned.
When multiple errors exist, the first encountered by parsing is reported.

## Rules owned by other modules

Clip rounding: defined in src/audio.rs (TL6).
Video tie rule: defined in src/video.rs (TL7).
Those modules are currently reserved; this format does not define either rule.
