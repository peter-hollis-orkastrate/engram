//! Safety gate for content filtering and PII redaction.
//!
//! Inspects captured content before it enters the ingestion pipeline,
//! detecting and optionally redacting sensitive information such as
//! credit card numbers, SSNs, and email addresses.
//!
//! Ported from OSpipe's proven safety gate pattern.

use crate::config::SafetyConfig;

/// Type of PII detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PiiType {
    Email,
    CreditCard,
    Ssn,
    PhoneNumber,
}

/// A detected PII match with position and redaction token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiiMatch {
    pub pii_type: PiiType,
    pub start: usize,
    pub end: usize,
    pub redacted_token: &'static str,
}

/// Decision made by the safety gate about a piece of content.
#[derive(Debug, Clone, PartialEq)]
pub enum SafetyDecision {
    /// Content is safe to store as-is.
    Allow,
    /// Content is safe after redaction; the redacted version is provided.
    Redacted {
        text: String,
        redaction_count: usize,
    },
    /// Content must not be stored.
    Deny { reason: String },
}

/// Safety gate that checks content for sensitive information.
pub struct SafetyGate {
    config: SafetyConfig,
}

impl SafetyGate {
    /// Create a new safety gate with the given configuration.
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    /// Check content and return a safety decision.
    ///
    /// If PII is detected and redaction is enabled, the content is
    /// returned in redacted form. If custom patterns match, the content
    /// is denied entirely.
    pub fn check(&self, content: &str) -> SafetyDecision {
        // Custom deny patterns checked first — deny takes priority.
        for pattern in &self.config.custom_deny_patterns {
            if content.contains(pattern.as_str()) {
                return SafetyDecision::Deny {
                    reason: format!("Custom deny pattern matched: {}", pattern),
                };
            }
        }

        let mut redacted = content.to_string();
        let mut total_redactions = 0usize;

        if self.config.credit_card_redaction {
            let (new_text, count) = redact_credit_cards(&redacted);
            if count > 0 {
                redacted = new_text;
                total_redactions += count;
            }
        }

        if self.config.ssn_redaction {
            let (new_text, count) = redact_ssns(&redacted);
            if count > 0 {
                redacted = new_text;
                total_redactions += count;
            }
        }

        if self.config.pii_detection {
            let (new_text, count) = redact_emails(&redacted);
            if count > 0 {
                redacted = new_text;
                total_redactions += count;
            }
        }

        if self.config.phone_redaction {
            let (new_text, count) = redact_phone_numbers(&redacted);
            if count > 0 {
                redacted = new_text;
                total_redactions += count;
            }
        }

        if total_redactions > 0 {
            SafetyDecision::Redacted {
                text: redacted,
                redaction_count: total_redactions,
            }
        } else {
            SafetyDecision::Allow
        }
    }

    /// Convenience: redact all detected sensitive content and return the cleaned string.
    pub fn redact(&self, content: &str) -> String {
        match self.check(content) {
            SafetyDecision::Allow => content.to_string(),
            SafetyDecision::Redacted { text, .. } => text,
            SafetyDecision::Deny { .. } => "[REDACTED]".to_string(),
        }
    }
}

/// Validate a sequence of digits using the Luhn checksum algorithm.
///
/// Returns `true` if the digit sequence has a valid Luhn checksum,
/// which is a necessary (but not sufficient) condition for a valid
/// credit card number.
fn luhn_check(digits: &[u32]) -> bool {
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Detect and redact sequences of 13-19 digits that look like credit card numbers.
///
/// Sequences of digits (with optional spaces or dashes) totaling 13-19 digits
/// are replaced with `[CC_REDACTED]` only if they pass the Luhn checksum.
fn redact_credit_cards(text: &str) -> (String, usize) {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut count = 0;

    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            let mut digit_count = 0;

            while i < chars.len()
                && (chars[i].is_ascii_digit() || chars[i] == ' ' || chars[i] == '-')
            {
                if chars[i].is_ascii_digit() {
                    digit_count += 1;
                }
                i += 1;
            }

            // Back up over trailing separators (spaces/dashes) that aren't
            // between digits — they belong to the surrounding text.
            while i > start && !chars[i - 1].is_ascii_digit() {
                i -= 1;
            }

            if (13..=19).contains(&digit_count) {
                // Collect just the digits for Luhn check
                let digit_values: Vec<u32> = chars[start..i]
                    .iter()
                    .filter_map(|c| c.to_digit(10))
                    .collect();
                if luhn_check(&digit_values) {
                    result.push_str("[CC_REDACTED]");
                    count += 1;
                } else {
                    for c in &chars[start..i] {
                        result.push(*c);
                    }
                }
            } else {
                for c in &chars[start..i] {
                    result.push(*c);
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    (result, count)
}

/// Detect and redact SSN patterns (XXX-XX-XXXX).
fn redact_ssns(text: &str) -> (String, usize) {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    let mut i = 0;

    while i < chars.len() {
        if i + 10 < chars.len() && is_ssn_at(&chars, i) {
            result.push_str("[SSN_REDACTED]");
            count += 1;
            i += 11; // XXX-XX-XXXX = 11 chars
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    (result, count)
}

/// Check if an SSN pattern exists at the given position.
fn is_ssn_at(chars: &[char], pos: usize) -> bool {
    if pos + 10 >= chars.len() {
        return false;
    }
    chars[pos].is_ascii_digit()
        && chars[pos + 1].is_ascii_digit()
        && chars[pos + 2].is_ascii_digit()
        && chars[pos + 3] == '-'
        && chars[pos + 4].is_ascii_digit()
        && chars[pos + 5].is_ascii_digit()
        && chars[pos + 6] == '-'
        && chars[pos + 7].is_ascii_digit()
        && chars[pos + 8].is_ascii_digit()
        && chars[pos + 9].is_ascii_digit()
        && chars[pos + 10].is_ascii_digit()
}

/// Detect and redact email addresses while preserving surrounding whitespace.
fn redact_emails(text: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(text.len());
    let mut count = 0;
    let mut i = 0;

    while i < len {
        if chars[i] == '@' {
            // Scan backwards for the local part.
            let mut local_start = i;
            while local_start > 0 && is_email_local_char(chars[local_start - 1]) {
                local_start -= 1;
            }

            // Scan forwards for the domain part.
            let mut domain_end = i + 1;
            let mut has_dot = false;
            while domain_end < len && is_email_domain_char(chars[domain_end]) {
                if chars[domain_end] == '.' {
                    has_dot = true;
                }
                domain_end += 1;
            }
            // Trim trailing dots/hyphens from domain.
            while domain_end > i + 1
                && (chars[domain_end - 1] == '.' || chars[domain_end - 1] == '-')
            {
                if chars[domain_end - 1] == '.' {
                    has_dot = chars[i + 1..domain_end - 1].contains(&'.');
                }
                domain_end -= 1;
            }

            let local_len = i - local_start;
            let domain_len = domain_end - (i + 1);

            if local_len > 0 && domain_len >= 3 && has_dot {
                // Truncate already-pushed local-part characters.
                let already_pushed = i - local_start;
                let new_len = result.len() - already_pushed;
                result.truncate(new_len);
                result.push_str("[EMAIL_REDACTED]");
                count += 1;
                i = domain_end;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    (result, count)
}

fn is_email_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-' || c == '_'
}

fn is_email_domain_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '-'
}

/// Detect and redact US phone numbers in various formats.
///
/// Supported formats:
/// - `(NNN) NNN-NNNN` and `(NNN)NNN-NNNN`
/// - `NNN-NNN-NNNN`
/// - `+1NNNNNNNNNN`
/// - `+1 NNN NNN NNNN`
/// - `NNN.NNN.NNNN`
///
/// Boundary guards prevent false positives on IP addresses, version numbers,
/// order numbers, part numbers, and URL path segments.
pub fn redact_phone_numbers(text: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(text.len());
    let mut count = 0;
    let mut i = 0;

    while i < len {
        // Try to match a phone number at position i.
        if let Some(end) = try_match_phone(&chars, i, len) {
            // Boundary check: character before must not be alphanumeric, '/', '#', or '@'.
            let bad_prefix = if i > 0 {
                let prev = chars[i - 1];
                prev.is_ascii_alphanumeric() || prev == '/' || prev == '#' || prev == '@'
            } else {
                false
            };

            // Boundary check: character after must not be alphanumeric
            // (punctuation like '.', ',', '!', '?' is allowed).
            let bad_suffix = if end < len {
                let next = chars[end];
                next.is_ascii_alphanumeric()
            } else {
                false
            };

            if bad_prefix || bad_suffix {
                result.push(chars[i]);
                i += 1;
            } else {
                result.push_str("[REDACTED-PHONE]");
                count += 1;
                i = end;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    (result, count)
}

/// Try to match a phone number starting at `pos`. Returns the end index if matched.
fn try_match_phone(chars: &[char], pos: usize, len: usize) -> Option<usize> {
    // Format: +1NNNNNNNNNN or +1 NNN NNN NNNN
    if chars[pos] == '+' && pos + 1 < len && chars[pos + 1] == '1' {
        // +1NNNNNNNNNN (12 chars: +1 followed by 10 digits)
        if pos + 12 <= len {
            let all_digits = (2..12).all(|j| chars[pos + j].is_ascii_digit());
            if all_digits {
                // Make sure this isn't part of a longer digit string
                if pos + 12 < len && chars[pos + 12].is_ascii_digit() {
                    return None;
                }
                return Some(pos + 12);
            }
        }
        // +1 NNN NNN NNNN (15 chars: +1 space NNN space NNN space NNNN)
        if pos + 15 <= len
            && chars[pos + 2] == ' '
            && chars[pos + 3].is_ascii_digit()
            && chars[pos + 4].is_ascii_digit()
            && chars[pos + 5].is_ascii_digit()
            && chars[pos + 6] == ' '
            && chars[pos + 7].is_ascii_digit()
            && chars[pos + 8].is_ascii_digit()
            && chars[pos + 9].is_ascii_digit()
            && chars[pos + 10] == ' '
            && chars[pos + 11].is_ascii_digit()
            && chars[pos + 12].is_ascii_digit()
            && chars[pos + 13].is_ascii_digit()
            && chars[pos + 14].is_ascii_digit()
        {
            return Some(pos + 15);
        }
        return None;
    }

    // Format: (NNN) NNN-NNNN or (NNN)NNN-NNNN
    if chars[pos] == '(' {
        // Need at least (NNN)NNN-NNNN = 13 chars
        if pos + 13 <= len
            && chars[pos + 1].is_ascii_digit()
            && chars[pos + 2].is_ascii_digit()
            && chars[pos + 3].is_ascii_digit()
            && chars[pos + 4] == ')'
        {
            // (NNN) NNN-NNNN (14 chars)
            if pos + 14 <= len
                && chars[pos + 5] == ' '
                && chars[pos + 6].is_ascii_digit()
                && chars[pos + 7].is_ascii_digit()
                && chars[pos + 8].is_ascii_digit()
                && chars[pos + 9] == '-'
                && chars[pos + 10].is_ascii_digit()
                && chars[pos + 11].is_ascii_digit()
                && chars[pos + 12].is_ascii_digit()
                && chars[pos + 13].is_ascii_digit()
            {
                return Some(pos + 14);
            }
            // (NNN)NNN-NNNN (13 chars)
            if chars[pos + 5].is_ascii_digit()
                && chars[pos + 6].is_ascii_digit()
                && chars[pos + 7].is_ascii_digit()
                && chars[pos + 8] == '-'
                && chars[pos + 9].is_ascii_digit()
                && chars[pos + 10].is_ascii_digit()
                && chars[pos + 11].is_ascii_digit()
                && chars[pos + 12].is_ascii_digit()
            {
                return Some(pos + 13);
            }
        }
        return None;
    }

    // Digit-starting formats: NNN-NNN-NNNN or NNN.NNN.NNNN
    if chars[pos].is_ascii_digit() {
        // Need 3 digits first
        if pos + 2 < len && chars[pos + 1].is_ascii_digit() && chars[pos + 2].is_ascii_digit() {
            let sep_pos = pos + 3;
            if sep_pos < len {
                let sep = chars[sep_pos];
                if sep == '-' || sep == '.' {
                    // NNN{sep}NNN{sep}NNNN
                    if sep_pos + 8 <= len
                        && chars[sep_pos + 1].is_ascii_digit()
                        && chars[sep_pos + 2].is_ascii_digit()
                        && chars[sep_pos + 3].is_ascii_digit()
                        && chars[sep_pos + 4] == sep
                        && chars[sep_pos + 5].is_ascii_digit()
                        && chars[sep_pos + 6].is_ascii_digit()
                        && chars[sep_pos + 7].is_ascii_digit()
                        && chars[sep_pos + 8].is_ascii_digit()
                    {
                        let end = sep_pos + 9;

                        // For dot-separated: guard against IP addresses (4 groups)
                        // and version numbers. IP = N+.N+.N+.N+ (4 dot-separated groups).
                        if sep == '.' {
                            // Check if there's a 4th dot-separated group after.
                            if end < len && chars[end] == '.' {
                                return None; // Likely IP or version
                            }
                            // Check if there's a dot-separated group before.
                            if pos > 0 && chars[pos - 1] == '.' {
                                return None; // Likely IP or version
                            }
                        }

                        return Some(end);
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_gate() -> SafetyGate {
        SafetyGate::new(SafetyConfig::default())
    }

    // -- Credit card redaction --

    #[test]
    fn test_redact_credit_card_with_dashes() {
        let gate = default_gate();
        let decision = gate.check("pay with 4111-1111-1111-1111 please");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "pay with [CC_REDACTED] please");
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_redact_credit_card_with_spaces() {
        let gate = default_gate();
        let decision = gate.check("card 4111 1111 1111 1111 end");
        match decision {
            SafetyDecision::Redacted { text, .. } => {
                assert_eq!(text, "card [CC_REDACTED] end");
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_short_numbers_not_redacted() {
        let gate = default_gate();
        let decision = gate.check("order 12345 confirmed");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    // -- SSN redaction --

    #[test]
    fn test_redact_ssn() {
        let gate = default_gate();
        let decision = gate.check("my ssn is 123-45-6789");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "my ssn is [SSN_REDACTED]");
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_partial_ssn_not_redacted() {
        let gate = default_gate();
        let decision = gate.check("phone 123-45-678");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    // -- Email redaction --

    #[test]
    fn test_redact_email() {
        let gate = default_gate();
        let decision = gate.check("contact user@example.com for info");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "contact [EMAIL_REDACTED] for info");
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_redact_multiple_emails() {
        let gate = default_gate();
        let decision = gate.check("a@b.com and c@d.org");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "[EMAIL_REDACTED] and [EMAIL_REDACTED]");
                assert_eq!(redaction_count, 2);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_email_preserves_whitespace() {
        let gate = default_gate();
        let decision = gate.check("contact\tuser@example.com\nhere");
        match decision {
            SafetyDecision::Redacted { text, .. } => {
                assert_eq!(text, "contact\t[EMAIL_REDACTED]\nhere");
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    // -- Custom deny patterns --

    #[test]
    fn test_custom_deny_pattern() {
        let config = SafetyConfig {
            custom_deny_patterns: vec!["password".to_string()],
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        let decision = gate.check("my password is secret123");
        match decision {
            SafetyDecision::Deny { reason } => {
                assert!(reason.contains("password"));
            }
            other => panic!("Expected Deny, got {:?}", other),
        }
    }

    // -- Allow --

    #[test]
    fn test_clean_text_allowed() {
        let gate = default_gate();
        let decision = gate.check("the weather is nice today");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    // -- Combined redactions --

    #[test]
    fn test_multiple_redaction_types() {
        let gate = default_gate();
        let decision = gate.check("ssn 123-45-6789 and email user@test.com");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert!(text.contains("[SSN_REDACTED]"));
                assert!(text.contains("[EMAIL_REDACTED]"));
                assert_eq!(redaction_count, 2);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    // -- Redact convenience method --

    #[test]
    fn test_redact_method_clean() {
        let gate = default_gate();
        assert_eq!(gate.redact("clean text"), "clean text");
    }

    #[test]
    fn test_redact_method_with_pii() {
        let gate = default_gate();
        let result = gate.redact("email user@example.com");
        assert_eq!(result, "email [EMAIL_REDACTED]");
    }

    #[test]
    fn test_redact_method_denied() {
        let config = SafetyConfig {
            custom_deny_patterns: vec!["secret".to_string()],
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        assert_eq!(gate.redact("this is secret data"), "[REDACTED]");
    }

    // -- Config toggles --

    #[test]
    fn test_disabled_cc_redaction() {
        let config = SafetyConfig {
            credit_card_redaction: false,
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        let decision = gate.check("card 4111-1111-1111-1111");
        // CC redaction disabled, no email or SSN present → Allow
        assert_eq!(decision, SafetyDecision::Allow);
    }

    #[test]
    fn test_disabled_ssn_redaction() {
        let config = SafetyConfig {
            ssn_redaction: false,
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        let decision = gate.check("ssn 123-45-6789");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    #[test]
    fn test_disabled_pii_detection() {
        let config = SafetyConfig {
            pii_detection: false,
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        let decision = gate.check("email user@example.com");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    // -- Luhn validation --

    #[test]
    fn test_luhn_valid_visa() {
        // 4111111111111111 is a well-known Visa test number that passes Luhn.
        let digits: Vec<u32> = "4111111111111111"
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        assert!(luhn_check(&digits));
    }

    #[test]
    fn test_luhn_valid_mastercard() {
        // 5500000000000004 is a well-known Mastercard test number that passes Luhn.
        let digits: Vec<u32> = "5500000000000004"
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        assert!(luhn_check(&digits));
    }

    #[test]
    fn test_luhn_invalid() {
        // 4111111111111112 does NOT pass Luhn, so it should not be redacted.
        let gate = default_gate();
        let decision = gate.check("card 4111111111111112 end");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    #[test]
    fn test_luhn_19_digit_card() {
        // 6304000000000000018 is a valid 19-digit Maestro test number.
        // Luhn check: passes (sum = 30).
        let digits: Vec<u32> = "6304000000000000018"
            .chars()
            .map(|c| c.to_digit(10).unwrap())
            .collect();
        assert!(luhn_check(&digits), "19-digit number should pass Luhn");

        let gate = default_gate();
        let decision = gate.check("card 6304000000000000018 end");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "card [CC_REDACTED] end");
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_luhn_rejects_random_digits() {
        // 1234567890123456 does not pass Luhn and should not be redacted.
        let gate = default_gate();
        let decision = gate.check("number 1234567890123456 end");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    // =========================================================================
    // Phone number redaction tests
    // =========================================================================

    // --- Format detection ---

    #[test]
    fn test_phone_parens_space() {
        let (text, count) = redact_phone_numbers("call (555) 123-4567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_parens_no_space() {
        let (text, count) = redact_phone_numbers("call (555)123-4567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_dashes() {
        let (text, count) = redact_phone_numbers("call 555-123-4567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_plus_one() {
        let (text, count) = redact_phone_numbers("call +15551234567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_plus_one_spaces() {
        let (text, count) = redact_phone_numbers("call +1 555 123 4567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_dots() {
        let (text, count) = redact_phone_numbers("call 555.123.4567 now");
        assert_eq!(text, "call [REDACTED-PHONE] now");
        assert_eq!(count, 1);
    }

    // --- Multiple phones ---

    #[test]
    fn test_phone_multiple() {
        let (text, count) =
            redact_phone_numbers("home 555-123-4567 work (800) 555-1234");
        assert_eq!(
            text,
            "home [REDACTED-PHONE] work [REDACTED-PHONE]"
        );
        assert_eq!(count, 2);
    }

    #[test]
    fn test_phone_three_different_formats() {
        let (text, count) =
            redact_phone_numbers("a 555-111-2222, b (555)333-4444, c +15555556666");
        assert_eq!(
            text,
            "a [REDACTED-PHONE], b [REDACTED-PHONE], c [REDACTED-PHONE]"
        );
        assert_eq!(count, 3);
    }

    // --- Position tests ---

    #[test]
    fn test_phone_at_start() {
        let (text, count) = redact_phone_numbers("555-123-4567 is my number");
        assert_eq!(text, "[REDACTED-PHONE] is my number");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_at_end() {
        let (text, count) = redact_phone_numbers("my number is 555-123-4567");
        assert_eq!(text, "my number is [REDACTED-PHONE]");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_only() {
        let (text, count) = redact_phone_numbers("555-123-4567");
        assert_eq!(text, "[REDACTED-PHONE]");
        assert_eq!(count, 1);
    }

    // --- Punctuation neighbors ---

    #[test]
    fn test_phone_followed_by_period() {
        let (text, count) = redact_phone_numbers("Call 555-123-4567.");
        assert_eq!(text, "Call [REDACTED-PHONE].");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_followed_by_comma() {
        let (text, count) = redact_phone_numbers("Call 555-123-4567, please");
        assert_eq!(text, "Call [REDACTED-PHONE], please");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_followed_by_exclamation() {
        let (text, count) = redact_phone_numbers("Call 555-123-4567!");
        assert_eq!(text, "Call [REDACTED-PHONE]!");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_followed_by_question() {
        let (text, count) = redact_phone_numbers("Is it 555-123-4567?");
        assert_eq!(text, "Is it [REDACTED-PHONE]?");
        assert_eq!(count, 1);
    }

    // --- False positive guards ---

    #[test]
    fn test_no_redact_ip_address() {
        let (text, count) = redact_phone_numbers("server at 192.168.1.100");
        assert_eq!(text, "server at 192.168.1.100");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_ip_address_2() {
        let (text, count) = redact_phone_numbers("IP 10.0.0.1 is local");
        assert_eq!(text, "IP 10.0.0.1 is local");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_version_4_groups() {
        // 4 dot-separated groups should not be redacted
        let (text, count) = redact_phone_numbers("version 1.234.567.8901");
        assert_eq!(text, "version 1.234.567.8901");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_version_prefix_dot() {
        // A dot before NNN.NNN.NNNN means it's likely a version
        let (text, count) = redact_phone_numbers("v2.0.3.4567");
        assert_eq!(text, "v2.0.3.4567");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_url_path() {
        let (text, count) =
            redact_phone_numbers("http://api.com/users/5551234567/profile");
        // The /5551234567/ has a leading '/' so should not be redacted
        assert_eq!(text, "http://api.com/users/5551234567/profile");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_order_number() {
        let (text, count) = redact_phone_numbers("order #1234567890");
        assert_eq!(text, "order #1234567890");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_currency() {
        // $1,234,567.89 — no phone-shaped patterns here
        let (text, count) = redact_phone_numbers("price $1,234,567.89");
        assert_eq!(text, "price $1,234,567.89");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_part_number_prefix() {
        // A555-123-4567B — alphanumeric prefix and suffix
        let (text, count) = redact_phone_numbers("part A555-123-4567B");
        assert_eq!(text, "part A555-123-4567B");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_part_number_suffix_only() {
        let (text, count) = redact_phone_numbers("part 555-123-4567B");
        assert_eq!(text, "part 555-123-4567B");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_part_number_prefix_only() {
        let (text, count) = redact_phone_numbers("part A555-123-4567");
        assert_eq!(text, "part A555-123-4567");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_at_prefix() {
        let (text, count) = redact_phone_numbers("user @555-123-4567");
        assert_eq!(text, "user @555-123-4567");
        assert_eq!(count, 0);
    }

    // --- Config toggle ---

    #[test]
    fn test_phone_redaction_disabled() {
        let config = SafetyConfig {
            phone_redaction: false,
            ..Default::default()
        };
        let gate = SafetyGate::new(config);
        let decision = gate.check("call 555-123-4567 now");
        assert_eq!(decision, SafetyDecision::Allow);
    }

    #[test]
    fn test_phone_redaction_enabled_via_gate() {
        let gate = default_gate();
        let decision = gate.check("call 555-123-4567 now");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert_eq!(text, "call [REDACTED-PHONE] now");
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    // --- Combined with other PII types ---

    #[test]
    fn test_phone_and_email_combined() {
        let gate = default_gate();
        let decision =
            gate.check("email user@test.com phone 555-123-4567");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert!(text.contains("[EMAIL_REDACTED]"));
                assert!(text.contains("[REDACTED-PHONE]"));
                assert_eq!(redaction_count, 2);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_phone_and_ssn_combined() {
        let gate = default_gate();
        let decision =
            gate.check("ssn 123-45-6789 phone (555) 123-4567");
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert!(text.contains("[SSN_REDACTED]"));
                assert!(text.contains("[REDACTED-PHONE]"));
                assert_eq!(redaction_count, 2);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    #[test]
    fn test_phone_email_ssn_all_combined() {
        let gate = default_gate();
        let decision = gate.check(
            "ssn 123-45-6789, email a@b.com, phone 555-123-4567",
        );
        match decision {
            SafetyDecision::Redacted { text, redaction_count } => {
                assert!(text.contains("[SSN_REDACTED]"));
                assert!(text.contains("[EMAIL_REDACTED]"));
                assert!(text.contains("[REDACTED-PHONE]"));
                assert_eq!(redaction_count, 3);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
    }

    // --- Edge cases ---

    #[test]
    fn test_phone_empty_string() {
        let (text, count) = redact_phone_numbers("");
        assert_eq!(text, "");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_phone_no_phone() {
        let (text, count) = redact_phone_numbers("no phone numbers here");
        assert_eq!(text, "no phone numbers here");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_phone_short_numbers_not_matched() {
        let (text, count) = redact_phone_numbers("123-456 is too short");
        assert_eq!(text, "123-456 is too short");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_phone_parens_with_newline() {
        let (text, count) = redact_phone_numbers("line1\n(555) 123-4567\nline3");
        assert_eq!(text, "line1\n[REDACTED-PHONE]\nline3");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_in_sentence_context() {
        let (text, count) =
            redact_phone_numbers("Please reach us at (800) 555-1234 during business hours.");
        assert_eq!(
            text,
            "Please reach us at [REDACTED-PHONE] during business hours."
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_plus_one_at_start() {
        let (text, count) = redact_phone_numbers("+15551234567");
        assert_eq!(text, "[REDACTED-PHONE]");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_plus_one_spaces_at_end() {
        let (text, count) = redact_phone_numbers("fax: +1 800 555 1234");
        assert_eq!(text, "fax: [REDACTED-PHONE]");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_phone_dots_at_start() {
        let (text, count) = redact_phone_numbers("555.123.4567 is the number");
        assert_eq!(text, "[REDACTED-PHONE] is the number");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_no_redact_hash_prefix_dashes() {
        let (text, count) = redact_phone_numbers("#555-123-4567");
        assert_eq!(text, "#555-123-4567");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_no_redact_slash_prefix() {
        let (text, count) = redact_phone_numbers("/5551234567");
        // +1 format requires + prefix, this is just digits after /
        assert_eq!(text, "/5551234567");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_phone_pii_type_and_match() {
        // Test PiiType and PiiMatch structs exist and are usable
        let m = PiiMatch {
            pii_type: PiiType::PhoneNumber,
            start: 0,
            end: 12,
            redacted_token: "[REDACTED-PHONE]",
        };
        assert_eq!(m.pii_type, PiiType::PhoneNumber);
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 12);
        assert_eq!(m.redacted_token, "[REDACTED-PHONE]");
    }

    #[test]
    fn test_pii_type_variants() {
        assert_ne!(PiiType::Email, PiiType::CreditCard);
        assert_ne!(PiiType::Ssn, PiiType::PhoneNumber);
        let phone = PiiType::PhoneNumber;
        assert_eq!(phone, PiiType::PhoneNumber);
    }

    #[test]
    fn test_phone_consecutive() {
        let (text, count) =
            redact_phone_numbers("555-111-2222 555-333-4444");
        assert_eq!(text, "[REDACTED-PHONE] [REDACTED-PHONE]");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_phone_with_tab() {
        let (text, count) = redact_phone_numbers("tab\t555-123-4567\there");
        assert_eq!(text, "tab\t[REDACTED-PHONE]\there");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_no_redact_too_many_digits_plus_one() {
        // +1 followed by 11 digits (too many)
        let (text, count) = redact_phone_numbers("+155512345670");
        assert_eq!(text, "+155512345670");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_phone_redact_method() {
        let gate = default_gate();
        let result = gate.redact("call 555-123-4567");
        assert_eq!(result, "call [REDACTED-PHONE]");
    }
}
