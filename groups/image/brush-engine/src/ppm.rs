//! Strict, single-image, eight-bit binary PPM (P6) serialization.

use std::io::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid eight-bit P6 image")
}

fn raster_len(width: usize, height: usize) -> io::Result<usize> {
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(invalid)
}

/// Write a single P6 image, propagating all output failures.
pub fn write(mut output: impl Write, image: &Image) -> io::Result<()> {
    if raster_len(image.width, image.height)? != image.rgb.len() {
        return Err(invalid());
    }
    write!(output, "P6\n{} {}\n255\n", image.width, image.height)?;
    output.write_all(&image.rgb)
}

fn token<'a>(bytes: &'a [u8], position: &mut usize) -> io::Result<&'a [u8]> {
    loop {
        while bytes.get(*position).is_some_and(u8::is_ascii_whitespace) {
            *position += 1;
        }
        if bytes.get(*position) != Some(&b'#') {
            break;
        }
        while bytes.get(*position).is_some_and(|b| *b != b'\n') {
            *position += 1;
        }
    }
    let start = *position;
    while bytes
        .get(*position)
        .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'#')
    {
        *position += 1;
    }
    if start == *position {
        return Err(invalid());
    }
    Ok(&bytes[start..*position])
}

fn number(bytes: &[u8]) -> io::Result<usize> {
    bytes.iter().try_fold(0usize, |value, digit| {
        if !digit.is_ascii_digit() {
            return Err(invalid());
        }
        value
            .checked_mul(10)
            .and_then(|v| v.checked_add(usize::from(digit - b'0')))
            .ok_or_else(invalid)
    })
}

/// Parse an already bounded byte slice. No allocation is made until the exact
/// raster length is verified. Trailing bytes and non-255 maxima are rejected.
/// Exactly one whitespace separator follows maxval (CRLF is also accepted);
/// raster bytes, including whitespace and '#', are never treated as header.
pub fn read(bytes: &[u8]) -> io::Result<Image> {
    let mut position = 0;
    if token(bytes, &mut position)? != b"P6" {
        return Err(invalid());
    }
    let width = number(token(bytes, &mut position)?)?;
    let height = number(token(bytes, &mut position)?)?;
    if number(token(bytes, &mut position)?)? != 255 {
        return Err(invalid());
    }
    if !bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
        return Err(invalid());
    }
    if bytes.get(position..position + 2) == Some(b"\r\n") {
        position += 2;
    } else {
        position += 1;
    }
    let raster = &bytes[position..];
    if raster.len() != raster_len(width, height)? {
        return Err(invalid());
    }
    Ok(Image {
        width,
        height,
        rgb: raster.to_vec(),
    })
}
