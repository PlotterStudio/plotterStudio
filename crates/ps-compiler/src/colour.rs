//! Colour helpers for pen ordering.

/// sRGB electro-optical transfer: an 8-bit channel to linear light.
fn linear(c: u8) -> f64 {
    let v = f64::from(c) / 255.0;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        libm::pow((v + 0.055) / 1.055, 2.4)
    }
}

/// CIELAB lightness `L*` (0 black to 100 white) of an sRGB colour under D65.
#[must_use]
pub fn lightness([r, g, b]: [u8; 3]) -> f64 {
    let y = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    // CIE: a cube-root branch above (6/29)^3 and a linear branch below it.
    if y > 216.0 / 24_389.0 {
        116.0 * libm::cbrt(y) - 16.0
    } else {
        24_389.0 / 27.0 * y
    }
}

#[cfg(test)]
mod tests;
