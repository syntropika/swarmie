use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use serde_json::Value;

pub const REDACTED: &str = "[REDACTED]";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    OpenAiApiKey,
    AnthropicApiKey,
    GoogleApiKey,
    AwsAccessKey,
    GitHubToken,
    GitLabToken,
    SlackToken,
    StripeKey,
    HuggingFaceToken,
    JsonWebToken,
    PrivateKey,
    Assigned,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAiApiKey => "OpenAI API key",
            Self::AnthropicApiKey => "Anthropic API key",
            Self::GoogleApiKey => "Google API key",
            Self::AwsAccessKey => "AWS access key",
            Self::GitHubToken => "GitHub token",
            Self::GitLabToken => "GitLab token",
            Self::SlackToken => "Slack token",
            Self::StripeKey => "Stripe key",
            Self::HuggingFaceToken => "Hugging Face token",
            Self::JsonWebToken => "JSON Web Token",
            Self::PrivateKey => "private key",
            Self::Assigned => "assigned secret",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretMatch {
    pub kind: Kind,
    pub range: Range<usize>,
}

pub fn find(input: &str) -> impl Iterator<Item = SecretMatch> + '_ {
    // Bytes, not chars: every token character is ASCII, so a token starts
    // on a char boundary, and the bytes of any other character are skipped.
    let bytes = input.as_bytes();
    let mut at = 0;
    std::iter::from_fn(move || {
        while let Some(&byte) = bytes.get(at) {
            if !is_token_byte(byte) {
                at += 1;
                continue;
            }
            let rest = bytes.get(at..).unwrap_or_default();
            let len = rest
                .iter()
                .position(|&byte| !is_token_byte(byte))
                .unwrap_or(rest.len());
            let token = rest.get(..len).unwrap_or_default();
            if may_be_secret(token)
                && starts_token(input, at)
                && let Some(rest) = input.get(at..)
            {
                let found = secret_at(rest).map(|(kind, len)| (kind, 0..len));
                let found = found.or_else(|| Some((Kind::Assigned, assigned(rest)?)));
                if let Some((kind, range)) = found {
                    let range = at + range.start..at + range.end;
                    at = range.end;
                    return Some(SecretMatch { kind, range });
                }
            }
            // A token's tail starts no key, so skip to the token's end.
            at += len;
        }
        None
    })
}

pub fn has_secret(input: &str) -> bool {
    find(input).next().is_some()
}

pub fn text(input: &str) -> Cow<'_, str> {
    let mut found = find(input).peekable();
    if found.peek().is_none() {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut from = 0;
    for secret in found {
        out.push_str(input.get(from..secret.range.start).unwrap_or_default());
        out.push_str(REDACTED);
        from = secret.range.end;
    }
    out.push_str(input.get(from..).unwrap_or_default());
    Cow::Owned(out)
}

pub fn json_text(json: &str) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    // How much of `json` is in `out`.
    let mut copied = 0;
    let mut at = 0;
    while let Some(open) = json.get(at..).and_then(|rest| rest.find('"')) {
        let open = at + open;
        let Some((close, escaped)) = closing_quote(json, open + 1) else {
            break;
        };
        at = close + 1;
        let is_key = json
            .get(at..)
            .is_some_and(|rest| rest.trim_start().starts_with(':'));
        let Some(inner) = json.get(open + 1..close).filter(|_| !is_key) else {
            continue;
        };
        // The literal, quotes included, redacted; `None` when it holds no secret.
        let redacted = if escaped {
            if !escaped_may_hold_secret(inner.as_bytes()) {
                continue;
            }
            serde_json::from_str::<String>(json.get(open..=close).unwrap_or_default())
                .ok()
                .and_then(|decoded| match text(&decoded) {
                    Cow::Owned(redacted) => serde_json::to_string(&redacted).ok(),
                    Cow::Borrowed(_) => None,
                })
        } else {
            // No escape: the literal is its text, and REDACTED needs none.
            match text(inner) {
                Cow::Owned(redacted) => Some(format!("\"{redacted}\"")),
                Cow::Borrowed(_) => None,
            }
        };
        if let Some(redacted) = redacted {
            let out = out.get_or_insert_with(|| String::with_capacity(json.len()));
            out.push_str(json.get(copied..open).unwrap_or_default());
            out.push_str(&redacted);
            copied = close + 1;
        }
    }
    match out {
        Some(mut out) => {
            out.push_str(json.get(copied..).unwrap_or_default());
            Cow::Owned(out)
        }
        None => Cow::Borrowed(json),
    }
}

fn closing_quote(json: &str, from: usize) -> Option<(usize, bool)> {
    let bytes = json.as_bytes().get(from..)?;
    let mut escaped = false;
    let mut at = 0;
    loop {
        let next = bytes
            .get(at..)?
            .iter()
            .position(|&b| b == b'"' || b == b'\\')?;
        at += next;
        if bytes.get(at) == Some(&b'"') {
            return Some((from + at, escaped));
        }
        escaped = true;
        at += 2;
    }
}

fn escaped_may_hold_secret(raw: &[u8]) -> bool {
    let mut at = 0;
    while let Some(&byte) = raw.get(at) {
        if byte == b'\\' {
            if raw.get(at + 1) != Some(&b'u') {
                at += 2;
                continue;
            }
            let code = raw
                .get(at + 2..at + 6)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u32::from_str_radix(hex, 16).ok());
            if code.is_none_or(|code| code >= 0x20) {
                return true;
            }
            at += 6;
            continue;
        }
        if !is_token_byte(byte) {
            at += 1;
            continue;
        }
        let rest = raw.get(at..).unwrap_or_default();
        let len = rest
            .iter()
            .position(|&byte| !is_token_byte(byte))
            .unwrap_or(rest.len());
        if may_be_secret(rest.get(..len).unwrap_or_default()) {
            return true;
        }
        at += len;
    }
    false
}

pub fn json(value: &mut Value) {
    match value {
        Value::String(string) => {
            if let Cow::Owned(redacted) = text(string) {
                *string = redacted;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(json),
        Value::Object(fields) => fields.values_mut().for_each(json),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn secret_at(rest: &str) -> Option<(Kind, usize)> {
    if rest.starts_with("-----BEGIN ") {
        return private_key(rest).map(|len| (Kind::PrivateKey, len));
    }
    if let Some(len) = json_web_token(rest) {
        return Some((Kind::JsonWebToken, len));
    }
    let token = rest.get(..token_len(rest))?;
    if let Some(len) = aws_access_key(token) {
        return Some((Kind::AwsAccessKey, len));
    }
    let kind = TOKENS.iter().find_map(|&(prefix, min, kind)| {
        let tail = token.strip_prefix(prefix)?;
        is_plausible(tail, min).then_some(kind)
    })?;
    Some((kind, token.len()))
}

const TOKENS: &[(&str, usize, Kind)] = &[
    ("sk-ant-", 16, Kind::AnthropicApiKey),
    ("sk-proj-", 16, Kind::OpenAiApiKey),
    ("sk-svcacct-", 16, Kind::OpenAiApiKey),
    ("sk-", 32, Kind::OpenAiApiKey),
    ("AIza", 20, Kind::GoogleApiKey),
    ("github_pat_", 22, Kind::GitHubToken),
    ("ghp_", 30, Kind::GitHubToken),
    ("gho_", 30, Kind::GitHubToken),
    ("ghu_", 30, Kind::GitHubToken),
    ("ghs_", 30, Kind::GitHubToken),
    ("ghr_", 30, Kind::GitHubToken),
    ("glpat-", 20, Kind::GitLabToken),
    ("xoxa-", 10, Kind::SlackToken),
    ("xoxb-", 10, Kind::SlackToken),
    ("xoxp-", 10, Kind::SlackToken),
    ("xoxr-", 10, Kind::SlackToken),
    ("xoxs-", 10, Kind::SlackToken),
    ("sk_live_", 16, Kind::StripeKey),
    ("sk_test_", 16, Kind::StripeKey),
    ("rk_live_", 16, Kind::StripeKey),
    ("rk_test_", 16, Kind::StripeKey),
    ("hf_", 30, Kind::HuggingFaceToken),
];

const SECRET_NAMES: &[&str] = &[
    "SECRET",
    "TOKEN",
    "PASSWORD",
    "PASSWD",
    "API_KEY",
    "ACCESS_KEY",
    "SECRET_KEY",
    "PRIVATE_KEY",
];

fn assigned(rest: &str) -> Option<Range<usize>> {
    let name = rest.get(..token_len(rest))?;
    let upper_snake = name.starts_with(|ch: char| ch.is_ascii_uppercase())
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    let secret_named = SECRET_NAMES.iter().any(|end| {
        name.strip_suffix(end)
            .is_some_and(|head| head.is_empty() || head.ends_with('_'))
    });
    if !upper_snake || !secret_named {
        return None;
    }
    // A quoted name's closing quote, then `=` or `:`, spaces around it.
    let after = rest.get(name.len()..)?;
    let after = after.strip_prefix(['"', '\'']).unwrap_or(after);
    let after = after.trim_start_matches([' ', '\t']);
    let after = after.strip_prefix(['=', ':'])?;
    let after = after.trim_start_matches([' ', '\t']);
    let start = rest.len() - after.len();
    let (from, value) = match after.chars().next()? {
        quote @ ('"' | '\'') => {
            let inner = after.get(1..)?;
            (start + 1, inner.get(..inner.find(quote)?)?)
        }
        _ => (
            start,
            after.get(..after.find(ends_value).unwrap_or(after.len()))?,
        ),
    };
    let plausible = value.len() >= 8
        && value.chars().all(|ch| ch.is_ascii_graphic() || ch == ' ')
        && !value.starts_with('$')
        && !value.chars().all(|ch| ch.is_ascii_digit())
        && value != REDACTED
        && secret_at(value).is_none();
    plausible.then(|| from..from + value.len())
}

fn ends_value(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ',' | ';' | '&')
}

fn is_plausible(tail: &str, min: usize) -> bool {
    tail.len() >= min && tail.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn aws_access_key(token: &str) -> Option<usize> {
    let tail = token
        .strip_prefix("AKIA")
        .or_else(|| token.strip_prefix("ASIA"))?;
    let len = tail
        .chars()
        .take_while(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        .count();
    (len == 16).then_some(4 + len)
}

fn json_web_token(rest: &str) -> Option<usize> {
    let mut len = 0;
    for _ in 0..2 {
        let part = rest.get(len..)?;
        let segment = token_len(part);
        if !part.starts_with("eyJ") || segment < 10 || !part.get(segment..)?.starts_with('.') {
            return None;
        }
        len += segment + 1;
    }
    Some(len + token_len(rest.get(len..)?))
}

fn may_be_secret(token: &[u8]) -> bool {
    let first = token.first().copied().unwrap_or_default();
    let second = token.get(1).copied().unwrap_or_default();
    let shaped = matches!(
        (first, second),
        (b's' | b'r', b'k')
            | (b'A', b'I' | b'K' | b'S')
            | (b'g', b'h' | b'i' | b'l')
            | (b'x', b'o')
            | (b'h', b'f')
            | (b'e', b'y')
            | (b'-', b'-')
    );
    let named =
        first.is_ascii_uppercase() && matches!(token.last(), Some(b'T' | b'N' | b'D' | b'Y'));
    shaped || named
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn private_key(rest: &str) -> Option<usize> {
    let header = rest.get("-----BEGIN ".len()..)?;
    let label = header.split_once("-----")?.0;
    let is_key = label.ends_with("PRIVATE KEY")
        && label.chars().all(|ch| ch.is_ascii_uppercase() || ch == ' ');
    if !is_key {
        return None;
    }
    let end = format!("-----END {label}-----");
    Some(
        rest.find(end.as_str())
            .map_or(rest.len(), |at| at + end.len()),
    )
}

fn token_len(rest: &str) -> usize {
    rest.find(|ch| !is_token_char(ch)).unwrap_or(rest.len())
}

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn starts_token(input: &str, at: usize) -> bool {
    input
        .get(..at)
        .and_then(|before| before.chars().next_back())
        .is_none_or(|ch| !is_token_char(ch) && !matches!(ch, '+' | '/'))
}
