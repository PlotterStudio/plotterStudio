//! Number and text formatting for G-code.
//!
//! Everything is printed from integers with integer arithmetic, so float formatting can never
//! change the bytes of an output file.

/// Prints micrometres as millimetres with exactly three decimals (`-12345` becomes `-12.345`).
#[must_use]
pub fn um(value: i64) -> String {
    fixed3(value)
}

/// Prints milli-degrees as degrees with exactly three decimals.
#[must_use]
pub fn mdeg(value: i64) -> String {
    fixed3(value)
}

fn fixed3(value: i64) -> String {
    let negative = value < 0;
    let magnitude = i128::from(value).unsigned_abs();
    let (whole, frac) = (magnitude / 1_000, magnitude % 1_000);
    format!("{}{whole}.{frac:03}", if negative { "-" } else { "" })
}

/// Like [`um`] but without trailing zeros, for SVG previews (`12.500` becomes `12.5`).
#[must_use]
pub fn um_trimmed(value: i64) -> String {
    let text = fixed3(value);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// Makes a message safe inside `M0 (...)`: newlines become `; ` and parentheses become square
/// brackets.
#[must_use]
pub fn pause_text(text: &str) -> String {
    let joined: Vec<&str> = text
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    joined.join("; ").replace('(', "[").replace(')', "]")
}

#[cfg(test)]
mod tests;
