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
}
