//! Finding and redacting secrets in text a host logs, shows or shares: API keys and tokens by
//! their known shapes, private key blocks, and values assigned to secret-named variables.
//!
//! A shape matches only where a token starts, so the same letters inside a word or a base64
//! blob are not a key. A secret with neither a known shape nor a telling name, such as a
//! password in prose, is not found.
//!
//! ```
//! use swarmie_tools::coding::redact;
//!
//! let line = "export ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
//! assert!(redact::has_secret(line));
//! assert_eq!(redact::text(line), "export ANTHROPIC_API_KEY=[REDACTED]");
//! assert_eq!(redact::text("nothing to hide"), "nothing to hide");
//! ```

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use serde_json::Value;

#[cfg(test)]
mod tests {
    //! What the rules find: each kind by its shape, where a token starts, and
    //! nothing that only looks like the start of a key.

    use serde_json::json;

    use super::*;

    /// One key of every kind, each found whole, and redacted once.
    #[test]
    fn every_kind_is_found_by_its_shape_and_redacted() {
        let keys = [
            (
                "sk-proj-abcdefghijklmnopqrstuvwxyz1234567890",
                Kind::OpenAiApiKey,
            ),
            ("sk-svcacct-abcdefghijklmnop1234", Kind::OpenAiApiKey),
            (
                "sk-abcdefghijklmnopqrstuvwxyz1234567890",
                Kind::OpenAiApiKey,
            ),
            (
                "sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890",
                Kind::AnthropicApiKey,
            ),
            ("AIzaSyExampleSecretToken1234567890", Kind::GoogleApiKey),
            ("AKIA1234567890ABCDEF", Kind::AwsAccessKey),
            ("ASIA0123456789ABCDEF", Kind::AwsAccessKey),
            (
                "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                Kind::GitHubToken,
            ),
            (
                "github_pat_11ABCDEFG0123456789_abcdefghij",
                Kind::GitHubToken,
            ),
            ("glpat-abcdefghij0123456789", Kind::GitLabToken),
            ("xoxb-1234567890-abcdefghij", Kind::SlackToken),
            ("sk_live_abcdefghij0123456789", Kind::StripeKey),
            (
                "hf_abcdefghijklmnopqrstuvwxyz0123456789",
                Kind::HuggingFaceToken,
            ),
            (
                "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJlLWJ5dGVz",
                Kind::JsonWebToken,
            ),
            (
                "eyJhbGciOiJub25lIn0.eyJzdWIiOiIxMjM0In0.",
                Kind::JsonWebToken,
            ),
            (
                "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk=\n-----END OPENSSH PRIVATE KEY-----",
                Kind::PrivateKey,
            ),
        ];
        for (key, kind) in keys {
            let line = format!("before \"{key}\", after");
            let found: Vec<SecretMatch> = find(&line).collect();
            let start = "before \"".len();
            assert_eq!(
                found,
                [SecretMatch {
                    kind,
                    range: start..start + key.len()
                }],
                "{key}"
            );
            let redacted = text(&line);
            assert_eq!(redacted, format!("before \"{REDACTED}\", after"));
            assert!(!has_secret(&redacted));
        }
    }

    /// Key prefixes inside a word or a base64 blob, and prefixes with a short
    /// or letterless tail, are not keys; text without one is not copied.
    #[test]
    fn only_a_whole_token_of_a_key_shape_is_a_key() {
        let harmless = [
            "the sk-etch marker is harmless",
            "sk-12345678901234567890123456789012345",
            "tool-file-id-page-two-verifier-8c27 file_011Cb1W1wnAxQP1a6AuVcPx5",
            "ErUBCkYIBRgCIkAIzaSyExampleSecretToken1234567890ABCDEFGHIJAKIA0123456789ABCDEFsk-proj-notakeyhereatall0123456789abcdef",
            "Eq/AIzaSyExampleSecretToken1234567890ABCDEFGHIJ+AKIA0123456789ABCDEF==",
            "AKIA0123456789ABCDEFG is one letter too long",
            "-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----",
            "eyJhbGciOiJSUzI1NiJ9 alone, or eyJhbGciOiJSUzI1NiJ9.eyJz.c2ln with a short payload",
            "theyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2ln inside a word",
        ];
        for line in harmless {
            assert!(matches!(text(line), Cow::Borrowed(_)), "{line}");
        }
    }

    /// Every string of a JSON value is redacted, keys and other values are
    /// kept, and a private key cut off mid-block is redacted to the end.
    #[test]
    fn json_redacts_every_string_and_a_cut_off_key_to_the_end() {
        let mut value = json!({
            "sk-abcdefghijklmnopqrstuvwxyz1234567890": 1,
            "headers": ["Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789", "plain"],
            "chunk": "key: -----BEGIN RSA PRIVATE KEY-----\nMIIEow",
            "count": 7,
        });
        json(&mut value);
        assert_eq!(
            value,
            json!({
                "sk-abcdefghijklmnopqrstuvwxyz1234567890": 1,
                "headers": [format!("Bearer {REDACTED}"), "plain"],
                "chunk": format!("key: {REDACTED}"),
                "count": 7,
            })
        );
    }

    /// A value assigned to a secret-named variable is redacted with the name
    /// kept; a count, a reference, a short value and a name that only holds a
    /// secret word are not secrets, and a known key keeps its own kind.
    #[test]
    fn a_value_assigned_to_a_secret_name_is_redacted_and_counts_are_not() {
        let line =
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY region=us-east-1";
        assert_eq!(
            text(line),
            "AWS_SECRET_ACCESS_KEY=[REDACTED] region=us-east-1"
        );
        assert_eq!(
            text(r#"{"DB_PASSWORD": "correct horse battery", "GITHUB_TOKEN": 'abcdefgh'}"#),
            r#"{"DB_PASSWORD": "[REDACTED]", "GITHUB_TOKEN": '[REDACTED]'}"#
        );
        assert_eq!(
            text("?API_KEY=abcdefghij&page=2"),
            "?API_KEY=[REDACTED]&page=2"
        );
        for kept in [
            "MAX_TOKENS=4096",
            "TOKEN_COUNT=12345678",
            "MAX_TOKENS_PER_CALL=unlimited",
            "TOKEN_BUDGET=generous-enough",
            "GH_TOKEN=$GITHUB_TOKEN",
            "API_KEY=short",
            "my_token=abcdefghijk",
            "API_KEY=[REDACTED]",
            "API_KEY=‹SECRET_1›",
        ] {
            assert_eq!(text(kept), kept);
        }
        let shaped: Vec<Kind> = find("GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789")
            .map(|secret| secret.kind)
            .collect();
        assert_eq!(shaped, [Kind::GitHubToken]);
    }

    /// Serialized JSON is redacted as its value is: escaped strings are
    /// decoded first, so a key after a newline or inside quotes is found; keys
    /// and clean text are left as they are, and the result still parses.
    #[test]
    fn json_text_redacts_serialized_json_as_its_value_is_redacted() {
        let value = json!({
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz1234567890": "a key as a name stays",
            "output": "PATH=/bin\nOPENAI_API_KEY=sk-test-probe-1234567890abcdef\nHOME=/home",
            "quoted": "export GITHUB_TOKEN=\"abcdefgh12345678\"",
            "token": "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJlLWJ5dGVz",
            "plain": ["no secret", "AKIA0123456789ABCDEF"],
            "n": 3,
        });
        let line = serde_json::to_string(&value).unwrap_or_default();
        let redacted = json_text(&line);
        let mut expected = value.clone();
        json(&mut expected);
        let parsed: serde_json::Value = serde_json::from_str(&redacted).unwrap_or_default();
        assert_eq!(parsed, expected);
        assert!(!redacted.contains("sk-test-probe"), "{redacted}");
        assert!(!redacted.contains("abcdefgh12345678"), "{redacted}");

        let clean = serde_json::to_string(&json!({"a": "line\none", "b": [1, "two"]}));
        let clean = clean.unwrap_or_default();
        assert!(matches!(json_text(&clean), Cow::Borrowed(_)));
        // A letter written as an escape is decoded before the search.
        let escaped = r#"{"a":"id \u0041KIA0123456789ABCDEF\n"}"#;
        assert_eq!(json_text(escaped), r#"{"a":"id [REDACTED]\n"}"#);
        let unclosed = r#"{"a":"AKIA0123456789ABCDEF"#;
        assert_eq!(json_text(unclosed), unclosed);
    }
}

/// What [`text`] and [`json`] put in place of a secret.
pub const REDACTED: &str = "[REDACTED]";

/// What kind of secret a [`SecretMatch`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// An OpenAI API key: `sk-`, `sk-proj-` or `sk-svcacct-`.
    OpenAiApiKey,
    /// An Anthropic API key: `sk-ant-`.
    AnthropicApiKey,
    /// A Google API key: `AIza`.
    GoogleApiKey,
    /// An AWS access key id: `AKIA` or `ASIA`.
    AwsAccessKey,
    /// A GitHub token: `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` or
    /// `github_pat_`.
    GitHubToken,
    /// A GitLab personal access token: `glpat-`.
    GitLabToken,
    /// A Slack token: `xoxa-`, `xoxb-`, `xoxp-`, `xoxr-` or `xoxs-`.
    SlackToken,
    /// A Stripe secret or restricted key: `sk_live_`, `sk_test_`,
    /// `rk_live_` or `rk_test_`.
    StripeKey,
    /// A Hugging Face access token: `hf_`.
    HuggingFaceToken,
    /// A JSON Web Token, such as an OAuth access or id token: a base64url
    /// header and payload, each a JSON object (`eyJ…`), and a signature,
    /// joined by `.`; the signature may be empty.
    JsonWebToken,
    /// A PEM private key block, `-----BEGIN … PRIVATE KEY-----` through its
    /// end line, or through the end of the text when it has none.
    PrivateKey,
    /// A value assigned to a name that says it is secret, `NAME=value` or
    /// `NAME: value`: the name in upper snake case and, or ending in,
    /// `SECRET`, `TOKEN`, `PASSWORD`, `PASSWD`, `API_KEY`, `ACCESS_KEY`,
    /// `SECRET_KEY` or `PRIVATE_KEY` (so `AWS_SECRET_ACCESS_KEY` and
    /// `GITHUB_TOKEN`, not `MAX_TOKENS` or `TOKEN_COUNT`). The value runs
    /// to its closing quote when quoted, else to whitespace, a quote, `,`,
    /// `;` or `&` (the next assignment of a query string); one of at most
    /// 7 characters, all digits, starting with `$` (a reference such as
    /// `$GITHUB_TOKEN`), with characters other than printable ASCII (a
    /// placeholder such as `‹SECRET_1›`), or [`REDACTED`] is not a secret.
    /// Only the value is the secret.
    Assigned,
}

impl Kind {
    /// The kind's name, such as `OpenAI API key`.
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

/// A secret [`find`] found: its kind, and where it is in the text, in
/// bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretMatch {
    /// What kind of secret it is.
    pub kind: Kind,
    /// Where it is in the text searched.
    pub range: Range<usize>,
}

/// Every secret in `input`, in order and not overlapping.
///
/// ```
/// use swarmie_tools::coding::redact::{self, Kind};
///
/// let found: Vec<_> = redact::find("id AKIA0123456789ABCDEF here").collect();
/// assert_eq!(found.len(), 1);
/// assert_eq!(found[0].kind, Kind::AwsAccessKey);
/// assert_eq!(found[0].range, 3..23);
/// ```
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

/// Whether `input` holds a secret.
pub fn has_secret(input: &str) -> bool {
    find(input).next().is_some()
}

/// `input` with every secret replaced by [`REDACTED`]; borrowed when it has
/// none.
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

/// `json`, serialized JSON such as a JSON line, with every string in it
/// redacted as [`json`] redacts a value: object keys are left as they are.
/// Borrowed when it has none, and only a string with an escape in it is
/// decoded to be searched. A string never closed is left as it is.
///
/// ```
/// use swarmie_tools::coding::redact;
///
/// let line = r#"{"text":"env\nOPENAI_API_KEY=sk-proj-abcdefghijklmnop1234"}"#;
/// assert_eq!(redact::json_text(line), r#"{"text":"env\nOPENAI_API_KEY=[REDACTED]"}"#);
/// assert!(matches!(redact::json_text(r#"{"a":"b"}"#), std::borrow::Cow::Borrowed(_)));
/// ```
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

/// Where the JSON string whose text starts at `from` in `json` closes, and
/// whether it has an escape; `None` when it never closes.
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

/// Whether the text of a JSON string, `raw` as written with its escapes,
/// may hold a secret, without decoding it: an escape decodes to a
/// character no token holds (a control character, a quote, `\\`, `/`), so
/// the decoded text's tokens are the runs between escapes, and none that
/// fails [`may_be_secret`] starts one. A `\u` escape of any other
/// character may, as may a string cut off mid-escape.
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

/// Every string in `value` redacted as [`text`] redacts it. Object keys
/// are left as they are.
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

/// The secret `rest` starts with, and its length in bytes.
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

/// Token shapes: a prefix, the fewest characters after it, and the kind. A
/// longer prefix comes before a shorter one it starts with.
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

/// The names, or ends of names after a `_`, that say a variable holds a
/// secret.
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

/// Where the secret value assigned to the secret-named variable `rest`
/// starts with is, in `rest`: see [`Kind::Assigned`]. `None` when the value
/// is a key of a known shape, which the scan finds as its own kind.
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

/// Whether `ch` ends an unquoted assigned value: whitespace, a quote, or a
/// separator between assignments (`,`, `;`, `&`).
fn ends_value(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ',' | ';' | '&')
}

/// Whether `tail`, the rest of a token after a key's prefix, is long
/// enough and has a letter, so `sk-` followed by a number or a short word
/// is not a key.
fn is_plausible(tail: &str, min: usize) -> bool {
    tail.len() >= min && tail.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// The length of the AWS access key id `token` starts with: `AKIA` or
/// `ASIA` and 16 upper-case letters or digits, no more.
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

/// The length of the JSON Web Token `rest` starts with: a header and a
/// payload of at least 10 base64url characters each, both starting `eyJ`
/// (`{"`), each followed by `.`, then the signature's base64url characters,
/// if any.
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

/// Whether a secret may start at `token`, a run of token characters: it
/// starts as a key's prefix, a private key's header or a JSON Web Token
/// does, or it is upper case and ends as a secret name does. A cheap test
/// before the full one.
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

/// Whether `byte` may be part of a key: [`is_token_char`] for bytes.
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

/// The length of the private key block `rest` starts with, through its end
/// line, else through the end of `rest`.
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

/// The length of the run of token characters `rest` starts with.
fn token_len(rest: &str) -> usize {
    rest.find(|ch| !is_token_char(ch)).unwrap_or(rest.len())
}

/// Whether `ch` may be part of a key.
fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

/// Whether a token starts at `at` in `input`: the character before is
/// neither a token character nor base64's `+` or `/`, so the same letters
/// inside a word or a base64 blob (a signature, ciphertext, an image) are
/// not read as the start of a key.
fn starts_token(input: &str, at: usize) -> bool {
    input
        .get(..at)
        .and_then(|before| before.chars().next_back())
        .is_none_or(|ch| !is_token_char(ch) && !matches!(ch, '+' | '/'))
}
