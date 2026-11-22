//! A parser for the `ps-gcode/1` dialect.
//!
//! The device host parses every line once and hands plugins both the exact text and this parsed
//! form, so two plugins can never disagree about what a line means. The dialect is fixed and the
//! parser refuses anything outside it. Coordinates stay exact integers in µm (read from the
//! writer's three-decimal text without any float); the feed is modal, as in the file.

use ps_core::PtUm;

/// What one line of a program means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A comment or blank line.
    Comment(String),
    /// `G20` or `G21`.
    Units {
        /// True for millimetres.
        millimetres: bool,
    },
    /// `G90`.
    Absolute,
    /// `G0`: a rapid move.
    Rapid {
        /// Destination in µm.
        to: PtUm,
    },
    /// `G1`: a drawn line.
    Linear {
        /// Destination in µm.
        to: PtUm,
        /// Tangent angle in milli-degrees, when the line has an `A` word.
        a_mdeg: Option<i64>,
        /// The feed in force, in mm/min.
        feed_mm_min: u32,
    },
    /// `G2`/`G3`: a drawn arc.
    Arc {
        /// Destination in µm.
        to: PtUm,
        /// Centre relative to the start, in µm (`I`, `J`).
        centre_offset: PtUm,
        /// True for `G3`.
        ccw: bool,
        /// The feed in force, in mm/min.
        feed_mm_min: u32,
    },
    /// `G4 P`: wait.
    Dwell {
        /// Milliseconds.
        ms: u32,
    },
    /// `M0`: stop until the user continues.
    Pause(String),
    /// `T<n> M6`: select a tool.
    ToolChange {
        /// The slot.
        slot: u8,
    },
    /// `M3 S<n>`: lower the tool with a force in machine units.
    ToolDown {
        /// The force.
        force: u32,
    },
    /// `M5`: raise the tool.
    ToolUp,
    /// `M104`/`M109`: set a heater temperature, optionally waiting.
    Temperature {
        /// Target in °C.
        celsius: i32,
        /// Whether to wait for it (`M109`).
        wait: bool,
    },
    /// `M2`: end of program.
    End,
}

/// One parsed line of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLine {
    /// The 1-based line number.
    pub number: u32,
    /// The line exactly as written.
    pub text: String,
    /// What it means.
    pub command: Command,
}

/// A line the parser refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
    /// The 1-based line number (0 when parsing a single line).
    pub line: u32,
    /// What is wrong.
    pub message: String,
}

fn fail(message: impl Into<String>) -> ParseError {
    ParseError {
        line: 0,
        message: message.into(),
    }
}

/// The state that carries from line to line: the modal feed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseState {
    feed: Option<u32>,
}

/// Parses a decimal with at most three decimals into thousandths, exactly.
fn thousandths(text: &str) -> Result<i64, ParseError> {
    let (negative, digits) = text.strip_prefix('-').map_or((false, text), |r| (true, r));
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() || frac.len() > 3 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fail(format!(
            "{text:?} is not a number with at most three decimals"
        )));
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fail(format!("{text:?} is not a number")));
    }
    let whole: i64 = whole
        .parse()
        .map_err(|_| fail(format!("{text:?} is too large")))?;
    let frac_value: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<3}")
            .parse()
            .map_err(|_| fail(format!("{text:?} is not a number")))?
    };
    let value = whole
        .checked_mul(1_000)
        .and_then(|v| v.checked_add(frac_value))
        .ok_or_else(|| fail(format!("{text:?} is too large")))?;
    Ok(if negative { -value } else { value })
}

fn unsigned(text: &str, what: &str) -> Result<u32, ParseError> {
    text.parse()
        .map_err(|_| fail(format!("{what} {text:?} is not a whole number")))
}

/// The words of a line as (letter, value) pairs.
fn words(body: &str) -> Result<Vec<(char, &str)>, ParseError> {
    body.split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            let letter = chars.next().ok_or_else(|| fail("empty word"))?;
            if !letter.is_ascii_uppercase() {
                return Err(fail(format!("{w:?} is not a G-code word")));
            }
            Ok((letter, &w[1..]))
        })
        .collect()
}

fn find<'a>(ws: &[(char, &'a str)], letter: char) -> Option<&'a str> {
    ws.iter().find(|(l, _)| *l == letter).map(|(_, v)| *v)
}

fn require<'a>(ws: &[(char, &'a str)], letter: char, code: &str) -> Result<&'a str, ParseError> {
    find(ws, letter).ok_or_else(|| fail(format!("{code} needs {letter}")))
}

fn point(ws: &[(char, &str)], x: char, y: char, code: &str) -> Result<PtUm, ParseError> {
    Ok(PtUm::new(
        thousandths(require(ws, x, code)?)?,
        thousandths(require(ws, y, code)?)?,
    ))
}

fn only(ws: &[(char, &str)], allowed: &str, code: &str) -> Result<(), ParseError> {
    match ws.iter().find(|(l, _)| !allowed.contains(*l)) {
        Some((l, _)) => Err(fail(format!("{code} does not take {l}"))),
        None => Ok(()),
    }
}

/// Parses one line, updating the modal feed.
///
/// # Errors
///
/// [`ParseError`] for anything outside the `ps-gcode/1` dialect.
pub fn parse_line(text: &str, state: &mut ParseState) -> Result<Command, ParseError> {
    let trimmed = text.trim();
    if let Some(comment) = trimmed.strip_prefix(';') {
        return Ok(Command::Comment(comment.trim().to_owned()));
    }
    if trimmed.is_empty() {
        return Ok(Command::Comment(String::new()));
    }
    // A pause message is in parentheses and may itself contain `;`, so it is read first.
    if trimmed == "M0" {
        return Ok(Command::Pause(String::new()));
    }
    if let Some(rest) = trimmed.strip_prefix("M0 (")
        && let Some(message) = rest.strip_suffix(')')
    {
        return Ok(Command::Pause(message.to_owned()));
    }

    let body = trimmed.split(';').next().unwrap_or("").trim();
    let ws = words(body)?;
    let Some(&(head, value)) = ws.first() else {
        return Ok(Command::Comment(String::new()));
    };
    let code = &body[..body.find(char::is_whitespace).unwrap_or(body.len())];
    let rest = &ws[1..];
    match (head, value) {
        ('G', "0") => {
            only(rest, "XY", code)?;
            Ok(Command::Rapid {
                to: point(rest, 'X', 'Y', code)?,
            })
        }
        ('G', "1") => {
            only(rest, "XYAF", code)?;
            if let Some(f) = find(rest, 'F') {
                state.feed = Some(unsigned(f, "F")?);
            }
            let feed_mm_min = state
                .feed
                .ok_or_else(|| fail("G1 before any feed (F) was set"))?;
            let a_mdeg = find(rest, 'A').map(thousandths).transpose()?;
            Ok(Command::Linear {
                to: point(rest, 'X', 'Y', code)?,
                a_mdeg,
                feed_mm_min,
            })
        }
        ('G', "2" | "3") => {
            only(rest, "XYIJF", code)?;
            if let Some(f) = find(rest, 'F') {
                state.feed = Some(unsigned(f, "F")?);
            }
            let feed_mm_min = state
                .feed
                .ok_or_else(|| fail(format!("{code} before any feed (F) was set")))?;
            Ok(Command::Arc {
                to: point(rest, 'X', 'Y', code)?,
                centre_offset: point(rest, 'I', 'J', code)?,
                ccw: value == "3",
                feed_mm_min,
            })
        }
        ('G', "4") => {
            only(rest, "P", code)?;
            Ok(Command::Dwell {
                ms: unsigned(require(rest, 'P', code)?, "P")?,
            })
        }
        ('G', "20") => Ok(Command::Units { millimetres: false }),
        ('G', "21") => Ok(Command::Units { millimetres: true }),
        ('G', "90") => Ok(Command::Absolute),
        ('M', "2") => Ok(Command::End),
        ('M', "3") => {
            only(rest, "S", code)?;
            Ok(Command::ToolDown {
                force: unsigned(require(rest, 'S', code)?, "S")?,
            })
        }
        ('M', "5") => Ok(Command::ToolUp),
        ('M', "104" | "109") => {
            only(rest, "S", code)?;
            let celsius = require(rest, 'S', code)?
                .parse::<i32>()
                .map_err(|_| fail(format!("{code} needs a whole-number S")))?;
            Ok(Command::Temperature {
                celsius,
                wait: value == "109",
            })
        }
        ('T', slot) => {
            if rest.len() == 1 && rest[0] == ('M', "6") {
                let slot = slot
                    .parse::<u8>()
                    .map_err(|_| fail(format!("T{slot} is not a tool slot")))?;
                Ok(Command::ToolChange { slot })
            } else {
                Err(fail(format!("{code} must be followed by M6")))
            }
        }
        _ => Err(fail(format!(
            "{code} is not part of the ps-gcode/1 dialect"
        ))),
    }
}

/// Parses a whole program.
///
/// # Errors
///
/// The first [`ParseError`], carrying its 1-based line number.
pub fn parse_program(text: &str) -> Result<Vec<ParsedLine>, ParseError> {
    let mut state = ParseState::default();
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let line = raw.trim_end_matches('\r');
        let command = parse_line(line, &mut state).map_err(|e| ParseError {
            line: number,
            message: e.message,
        })?;
        out.push(ParsedLine {
            number,
            text: line.to_owned(),
            command,
        });
    }
    Ok(out)
}

/// The line number of the program's `M2`, if it has one.
#[must_use]
pub fn end_line(lines: &[ParsedLine]) -> Option<u32> {
    lines
        .iter()
        .find(|l| l.command == Command::End)
        .map(|l| l.number)
}

/// The pauses of a program with their line numbers and messages.
#[must_use]
pub fn pause_lines(lines: &[ParsedLine]) -> Vec<(u32, &str)> {
    lines
        .iter()
        .filter_map(|l| match &l.command {
            Command::Pause(text) => Some((l.number, text.as_str())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
