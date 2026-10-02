use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Num(u64),
    Alpha(String),
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_num: Option<bool> = None;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            let is_num = ch.is_ascii_digit();
            match current_is_num {
                Some(prev) if prev != is_num => {
                    push_token(&mut tokens, &current, prev);
                    current.clear();
                }
                _ => {}
            }
            current.push(ch);
            current_is_num = Some(is_num);
        } else if !current.is_empty() {
            push_token(&mut tokens, &current, current_is_num.unwrap_or(false));
            current.clear();
            current_is_num = None;
        }
    }
    if !current.is_empty() {
        push_token(&mut tokens, &current, current_is_num.unwrap_or(false));
    }
    tokens
}

fn push_token(tokens: &mut Vec<Token>, text: &str, is_num: bool) {
    if is_num {
        let value = text.parse::<u64>().unwrap_or(u64::MAX);
        tokens.push(Token::Num(value));
    } else {
        tokens.push(Token::Alpha(text.to_ascii_lowercase()));
    }
}

/// Compares two version-like strings using natural ordering.
///
/// Numeric chunks are compared numerically, alphanumeric chunks
/// lexicographically. A numeric chunk is considered greater than a
/// non-numeric chunk at the same position, so `1.0.0` sorts after
/// `1.0.0-beta`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let ta = tokenize(a);
    let tb = tokenize(b);
    let mut ia = ta.iter();
    let mut ib = tb.iter();

    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            // A trailing alphanumeric chunk denotes a pre-release, so the
            // shorter version sorts higher (e.g. 1.0.0 > 1.0.0-beta).
            (None, Some(Token::Alpha(_))) => return Ordering::Greater,
            (None, Some(Token::Num(_))) => return Ordering::Less,
            (Some(Token::Alpha(_)), None) => return Ordering::Less,
            (Some(Token::Num(_)), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = match (x, y) {
                    (Token::Num(n1), Token::Num(n2)) => n1.cmp(n2),
                    (Token::Alpha(s1), Token::Alpha(s2)) => s1.cmp(s2),
                    (Token::Num(_), Token::Alpha(_)) => Ordering::Greater,
                    (Token::Alpha(_), Token::Num(_)) => Ordering::Less,
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// A parsed version of a mod, represented as its natural-order tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModVersionNumber {
    raw: String,
    tokens: Vec<Token>,
}

impl ModVersionNumber {
    pub fn parse(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
            tokens: tokenize(raw),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl Ord for ModVersionNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_versions(&self.raw, &other.raw)
    }
}

impl PartialOrd for ModVersionNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ModVersionNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Constraint {
    Gte(String),
    Gt(String),
    Lte(String),
    Lt(String),
    Eq(String),
}

impl Constraint {
    fn matches(&self, version: &str) -> bool {
        match self {
            Constraint::Gte(v) => compare_versions(version, v) != Ordering::Less,
            Constraint::Gt(v) => compare_versions(version, v) == Ordering::Greater,
            Constraint::Lte(v) => compare_versions(version, v) != Ordering::Greater,
            Constraint::Lt(v) => compare_versions(version, v) == Ordering::Less,
            Constraint::Eq(v) => compare_versions(version, v) == Ordering::Equal,
        }
    }
}

/// A version constraint expression.
///
/// Supports operator form (`>=1.20 <1.22`, `>=1.20,<1.22`), exact
/// versions (`1.20.1`), the wildcards `*`/`latest`, and Maven-style
/// bracket ranges (`[1.4,1.5)`, `(,1.0]`, `[1.0]`). Multiple bracket
/// ranges are treated as a union (OR); operator tokens form a
/// conjunction (AND).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    clauses: Vec<Vec<Constraint>>,
}

impl VersionRange {
    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed == "*" || trimmed.eq_ignore_ascii_case("latest") {
            return Ok(VersionRange {
                clauses: Vec::new(),
            });
        }
        if trimmed.contains('[') || trimmed.contains('(') {
            Self::parse_bracket_ranges(trimmed)
        } else {
            Self::parse_operator_ranges(trimmed)
        }
    }

    fn parse_operator_ranges(input: &str) -> Result<Self, String> {
        let tokens: Vec<&str> = input
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .collect();
        if tokens.is_empty() {
            return Ok(VersionRange {
                clauses: Vec::new(),
            });
        }
        let clause = tokens
            .into_iter()
            .map(Self::parse_operator_token)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VersionRange {
            clauses: vec![clause],
        })
    }

    fn parse_operator_token(token: &str) -> Result<Constraint, String> {
        let token = token.trim();
        if let Some(rest) = token.strip_prefix(">=") {
            Ok(Constraint::Gte(rest.trim().to_string()))
        } else if let Some(rest) = token.strip_prefix("<=") {
            Ok(Constraint::Lte(rest.trim().to_string()))
        } else if let Some(rest) = token.strip_prefix("==") {
            Ok(Constraint::Eq(rest.trim().to_string()))
        } else if let Some(rest) = token.strip_prefix('>') {
            Ok(Constraint::Gt(rest.trim().to_string()))
        } else if let Some(rest) = token.strip_prefix('<') {
            Ok(Constraint::Lt(rest.trim().to_string()))
        } else if let Some(rest) = token.strip_prefix('=') {
            Ok(Constraint::Eq(rest.trim().to_string()))
        } else {
            Ok(Constraint::Eq(token.to_string()))
        }
    }

    fn parse_bracket_ranges(input: &str) -> Result<Self, String> {
        let mut clauses = Vec::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '[' || c == '(' {
                let lower_inclusive = c == '[';
                let mut body = String::new();
                let mut closed = None;
                for next in chars.by_ref() {
                    if next == ']' || next == ')' {
                        closed = Some(next);
                        break;
                    }
                    body.push(next);
                }
                let upper = closed.ok_or_else(|| format!("unterminated range in '{}'", input))?;
                let upper_inclusive = upper == ']';

                let parts: Vec<&str> = body.splitn(2, ',').collect();
                if parts.len() == 1 {
                    let value = parts[0].trim();
                    if value.is_empty() {
                        return Err(format!("empty exact range in '{}'", input));
                    }
                    clauses.push(vec![Constraint::Eq(value.to_string())]);
                } else {
                    let mut clause = Vec::new();
                    let lower = parts[0].trim();
                    let upper_bound = parts[1].trim();
                    if !lower.is_empty() {
                        clause.push(if lower_inclusive {
                            Constraint::Gte(lower.to_string())
                        } else {
                            Constraint::Gt(lower.to_string())
                        });
                    }
                    if !upper_bound.is_empty() {
                        clause.push(if upper_inclusive {
                            Constraint::Lte(upper_bound.to_string())
                        } else {
                            Constraint::Lt(upper_bound.to_string())
                        });
                    }
                    clauses.push(clause);
                }
            }
        }

        if clauses.is_empty() {
            return Err(format!("invalid version range '{}'", input));
        }
        Ok(VersionRange { clauses })
    }

    pub fn matches(&self, version: &str) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        self.clauses
            .iter()
            .any(|clause| clause.iter().all(|c| c.matches(version)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_less(a: &str, b: &str) {
        assert_eq!(compare_versions(a, b), Ordering::Less, "{} < {}", a, b);
        assert_eq!(compare_versions(b, a), Ordering::Greater, "{} > {}", b, a);
    }

    #[test]
    fn test_numeric_ordering() {
        assert_less("0.5.8", "0.5.11");
        assert_less("1.20.1", "1.20.2");
        assert_less("1.9.0", "1.10.0");
    }

    #[test]
    fn test_release_after_prerelease() {
        assert_less("1.0.0-beta", "1.0.0");
        assert_less("1.0.0-alpha", "1.0.0-beta");
    }

    #[test]
    fn test_build_suffix() {
        assert_less("0.5.1.f", "0.5.2");
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn test_mod_version_number_ord() {
        let mut versions = [
            ModVersionNumber::parse("0.5.11"),
            ModVersionNumber::parse("0.5.8"),
            ModVersionNumber::parse("0.6.0"),
        ];
        versions.sort();
        assert_eq!(versions[0].as_str(), "0.5.8");
        assert_eq!(versions[2].as_str(), "0.6.0");
    }

    #[test]
    fn test_range_wildcards() {
        for spec in ["", "*", "latest", "LATEST"] {
            let range = VersionRange::parse(spec).unwrap();
            assert!(range.matches("1.20.1"), "{}", spec);
        }
    }

    #[test]
    fn test_range_operators() {
        let range = VersionRange::parse(">=1.20 <1.22").unwrap();
        assert!(range.matches("1.20"));
        assert!(range.matches("1.21.4"));
        assert!(!range.matches("1.22"));
        assert!(!range.matches("1.19.2"));
    }

    #[test]
    fn test_range_exact_and_equals() {
        assert!(VersionRange::parse("1.20.1").unwrap().matches("1.20.1"));
        assert!(!VersionRange::parse("1.20.1").unwrap().matches("1.20.2"));
        assert!(VersionRange::parse("=1.20.1").unwrap().matches("1.20.1"));
    }

    #[test]
    fn test_range_maven_brackets() {
        let range = VersionRange::parse("[1.4,1.5)").unwrap();
        assert!(range.matches("1.4"));
        assert!(range.matches("1.4.9"));
        assert!(!range.matches("1.5"));
        assert!(!range.matches("1.3"));

        assert!(VersionRange::parse("(,1.0]").unwrap().matches("0.9"));
        assert!(VersionRange::parse("(,1.0]").unwrap().matches("1.0"));
        assert!(!VersionRange::parse("(,1.0]").unwrap().matches("1.0.1"));

        assert!(VersionRange::parse("[2.0,)").unwrap().matches("2.5"));
        assert!(!VersionRange::parse("[2.0,)").unwrap().matches("1.9"));

        assert!(VersionRange::parse("[1.0]").unwrap().matches("1.0"));
        assert!(!VersionRange::parse("[1.0]").unwrap().matches("1.0.1"));
    }

    #[test]
    fn test_range_union() {
        let range = VersionRange::parse("[1.5,1.6),[1.7,)").unwrap();
        assert!(range.matches("1.5.2"));
        assert!(!range.matches("1.6.1"));
        assert!(range.matches("1.7.0"));
    }

    #[test]
    fn test_range_invalid() {
        assert!(VersionRange::parse("[1.0,").is_err());
    }
}
