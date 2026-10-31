//! Application text policy. Wire bytes and prepared model token budgets are separate.

const MAX_SCALARS: usize = 1_048_576;

/// Rust strings contain Unicode scalar values. Stop counting after the first
/// value outside the policy; never normalize or truncate the submitted text.
pub(super) fn validate_text(text: &str) -> tembra::Result<()> {
    if text.trim().is_empty() {
        return Err(tembra::Error::invalid(
            "conversation.text",
            "conversation text must not be empty",
        ));
    }
    if text.chars().take(MAX_SCALARS + 1).count() > MAX_SCALARS {
        return Err(tembra::Error::invalid(
            "conversation.text",
            "conversation text exceeds 1,048,576 Unicode scalar values",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_accepts_ascii_and_four_byte_scalars_without_a_byte_ceiling() {
        let ascii = "a".repeat(MAX_SCALARS);
        assert!(validate_text(&ascii).is_ok());
        let mut emoji = "🙂".repeat(MAX_SCALARS);
        assert_eq!(emoji.len(), 4 * MAX_SCALARS);
        assert!(validate_text(&emoji).is_ok());
        emoji.push('a');
        let refused = validate_text(&emoji).unwrap_err();
        assert_eq!(refused.code, tembra::Code::Invalid);
        assert_eq!(refused.reason, "conversation.text");
    }

    #[test]
    fn json_escape_expansion_does_not_change_the_text_policy_or_its_source() {
        let source = "\0".repeat(MAX_SCALARS);
        let encoded = serde_json::to_vec(&source).unwrap();
        assert_eq!(encoded.len(), 6 * MAX_SCALARS + 2);
        assert!(validate_text(&source).is_ok());
        let decoded: String = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, source);
        assert!(validate_text(&decoded).is_ok());
    }

    #[test]
    fn empty_or_whitespace_text_is_refused() {
        for text in ["", " \n\t", "\u{2003}"] {
            assert_eq!(validate_text(text).unwrap_err().code, tembra::Code::Invalid);
        }
    }
}
