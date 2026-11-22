//! `cargo xtask licenses`: checks every dependency licence against the allow-list
//! in `deny.toml` and writes the third-party notice that ships with the app.

/// A dependency as the licence check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) license: Option<String>,
}

/// The `[licenses] allow` list from the text of `deny.toml`.
pub(crate) fn allow_list(toml_text: &str) -> Result<Vec<String>, String> {
    let value: toml::Table = toml_text.parse().map_err(|e| format!("deny.toml: {e}"))?;
    let list = value
        .get("licenses")
        .and_then(|l| l.get("allow"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "deny.toml has no [licenses] allow list".to_owned())?;
    Ok(list
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect())
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    With,
    Id(String),
}

fn tokens(expr: &str) -> Vec<Token> {
    let spaced = expr
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace('/', " OR ");
    spaced
        .split_whitespace()
        .map(|w| match w {
            "(" => Token::Open,
            ")" => Token::Close,
            "AND" => Token::And,
            "OR" => Token::Or,
            "WITH" => Token::With,
            id => Token::Id(id.to_owned()),
        })
        .collect()
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    allow: &'a [String],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn or_expr(&mut self) -> bool {
        let mut ok = self.and_expr();
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            // Both sides are parsed so the position moves on; one allowed side is enough.
            let rhs = self.and_expr();
            ok = ok || rhs;
        }
        ok
    }

    fn and_expr(&mut self) -> bool {
        let mut ok = self.term();
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            let rhs = self.term();
            ok = ok && rhs;
        }
        ok
    }

    fn term(&mut self) -> bool {
        match self.tokens.get(self.at) {
            Some(Token::Open) => {
                self.at += 1;
                let ok = self.or_expr();
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                }
                ok
            }
            Some(Token::Id(id)) => {
                let id = id.clone();
                self.at += 1;
                let exception = if self.peek() == Some(&Token::With) {
                    self.at += 1;
                    match self.tokens.get(self.at) {
                        Some(Token::Id(e)) => {
                            let e = e.clone();
                            self.at += 1;
                            Some(e)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                match exception {
                    // An exception only adds permissions, so the plain licence being allowed is
                    // enough, as is the exact combination.
                    Some(e) => {
                        self.allow.contains(&format!("{id} WITH {e}")) || self.allow.contains(&id)
                    }
                    None => self.allow.contains(&id),
                }
            }
            _ => {
                self.at += 1;
                false
            }
        }
    }
}

/// True when the SPDX expression can be satisfied using only allowed licences.
pub(crate) fn satisfies(expr: &str, allow: &[String]) -> bool {
    let mut parser = Parser {
        tokens: tokens(expr),
        at: 0,
        allow,
    };
    parser.or_expr()
}

/// Licences that are not SPDX identifiers but are accepted, because their terms were read and are
/// met by the notice: the Hershey fonts allow any use if the acknowledgements travel with them.
const ACCEPTED_REFERENCES: [&str; 1] = ["LicenseRef-Hershey"];

/// Packages whose metadata names no licence but whose licence file does.
const CLARIFIED: [(&str, &str); 1] = [("vector-text-hershey", "LicenseRef-Hershey")];

/// The acknowledgements the Hershey fonts require to be distributed with them.
const HERSHEY_NOTICE: &str = "The Hershey Fonts were originally created by Dr. A. V. Hershey while working at the U. S. National Bureau of Standards. The format of the font data in this distribution was originally created by James Hurt, Cognition, Inc., 900 Technology Park Drive, Billerica, MA 01821.";

/// The acknowledgement for the typeface the app ships; its licence text travels with the font
/// files in `crates/ps-app/assets/fonts`.
const POPPINS_NOTICE: &str = "The app is set in Poppins, copyright 2020 The Poppins Project Authors (https://github.com/itfoundry/Poppins), used under the SIL Open Font License, Version 1.1. The licence text is in crates/ps-app/assets/fonts/Poppins-OFL.txt.";

/// Fills in the licence of packages that declare none but are known to carry a licence file.
pub(crate) fn clarify(deps: Vec<Dependency>) -> Vec<Dependency> {
    deps.into_iter()
        .map(|mut d| {
            if d.license.is_none()
                && let Some((_, expr)) = CLARIFIED.iter().find(|(name, _)| *name == d.name)
            {
                d.license = Some((*expr).to_owned());
            }
            d
        })
        .collect()
}

/// One message per dependency that is not allowed.
pub(crate) fn violations(deps: &[Dependency], allow: &[String]) -> Vec<String> {
    deps.iter()
        .filter_map(|d| match &d.license {
            None => Some(format!("{} {} declares no licence", d.name, d.version)),
            Some(l) if !ACCEPTED_REFERENCES.contains(&l.as_str()) && !satisfies(l, allow) => {
                Some(format!(
                    "{} {} is licensed {l}, which deny.toml does not allow",
                    d.name, d.version
                ))
            }
            Some(_) => None,
        })
        .collect()
}

/// The third-party notice: each package once, sorted, with its licence.
pub(crate) fn notice(deps: &[Dependency]) -> String {
    let mut lines: Vec<String> = deps
        .iter()
        .map(|d| {
            format!(
                "- {} {}: {}",
                d.name,
                d.version,
                d.license.as_deref().unwrap_or("no licence declared")
            )
        })
        .collect();
    lines.sort();
    lines.dedup();
    let hershey = if deps.iter().any(|d| d.name == "vector-text-hershey") {
        format!("\n## Hershey fonts\n\n{HERSHEY_NOTICE}\n")
    } else {
        String::new()
    };
    format!(
        "# Third-party licences\n\nPlotter Studio is built from these packages.\n\n{}\n{hershey}\n## Poppins\n\n{POPPINS_NOTICE}\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests;
