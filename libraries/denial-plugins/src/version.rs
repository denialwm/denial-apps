//! Dart's version constraints, as Pub writes them: `any`, `^1.2.3`, a bare
//! version, or comparisons such as `>=3.13.0 <4.0.0`.

use semver::Version;

use crate::util::{Error, Result};

pub fn parse_version(text: &str) -> Result<Version> {
    Version::parse(text.trim())
        .map_err(|_| Error::format(format!("Could not parse version \"{text}\".")))
}

#[derive(Debug)]
enum Bound {
    Exact(Version),
    Min(Version, bool),
    Max(Version, bool),
}

/// Whether `version` meets `constraint`.
pub fn allows(constraint: &str, version: &Version) -> Result<bool> {
    let bad = || {
        Error::format(format!(
            "Could not parse version constraint \"{constraint}\"."
        ))
    };
    let text = constraint.trim();
    if text == "any" {
        return Ok(true);
    }
    let mut bounds = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (operator, tail) = if let Some(tail) = rest.strip_prefix(">=") {
            (">=", tail)
        } else if let Some(tail) = rest.strip_prefix("<=") {
            ("<=", tail)
        } else if let Some(tail) = rest.strip_prefix('>') {
            (">", tail)
        } else if let Some(tail) = rest.strip_prefix('<') {
            ("<", tail)
        } else if let Some(tail) = rest.strip_prefix('^') {
            ("^", tail)
        } else {
            ("", rest)
        };
        let tail = tail.trim_start();
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let number = parse_version(&tail[..end]).map_err(|_| bad())?;
        rest = tail[end..].trim_start();
        match operator {
            ">=" => bounds.push(Bound::Min(number, true)),
            ">" => bounds.push(Bound::Min(number, false)),
            "<=" => bounds.push(Bound::Max(number, true)),
            "<" => bounds.push(Bound::Max(number, false)),
            "^" => {
                let next = if number.major == 0 {
                    Version::new(0, number.minor + 1, 0)
                } else {
                    Version::new(number.major + 1, 0, 0)
                };
                bounds.push(Bound::Min(number, true));
                bounds.push(Bound::Max(next, false));
            }
            _ => bounds.push(Bound::Exact(number)),
        }
    }
    if bounds.is_empty() {
        return Err(bad());
    }
    Ok(bounds.iter().all(|bound| match bound {
        Bound::Exact(wanted) => version == wanted,
        Bound::Min(min, inclusive) => version > min || (*inclusive && version == min),
        Bound::Max(max, inclusive) => version < max || (*inclusive && version == max),
    }))
}
