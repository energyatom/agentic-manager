//! Characters a terminal or a model's prompt would not show as themselves
//! (story 3.4, review rounds 1 and 2: R7, S4): one hand-written table, used
//! by the store's note check and by `agentic gate`'s display of model text.
//!
//! A character is *hidden* when it is in Unicode's Cc (a control character)
//! other than newline and tab, Zl or Zp (the line and paragraph
//! separators), or has the Default_Ignorable_Code_Point property (bidi
//! controls, zero-width characters, the byte-order mark, the variation
//! selectors, the Hangul fillers, the tag block, ...). Such a character can
//! reorder, hide or forge what a person reads, so text shown to the owner
//! escapes it, and a person's note may not hold it.

/// Default_Ignorable_Code_Point, as inclusive ranges, copied by hand from
/// the Unicode Character Database, `DerivedCoreProperties.txt`, Unicode
/// 16.0.0 (its "Derived Property: Default_Ignorable_Code_Point" section;
/// adjacent ranges merged). No crate: a table this small is read, not
/// fetched.
const DEFAULT_IGNORABLE: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),   // SOFT HYPHEN
    (0x034F, 0x034F),   // COMBINING GRAPHEME JOINER
    (0x061C, 0x061C),   // ARABIC LETTER MARK
    (0x115F, 0x1160),   // HANGUL CHOSEONG FILLER..HANGUL JUNGSEONG FILLER
    (0x17B4, 0x17B5),   // KHMER VOWEL INHERENT AQ..AA
    (0x180B, 0x180F),   // MONGOLIAN FREE VARIATION SELECTORS, VOWEL SEPARATOR
    (0x200B, 0x200F),   // ZERO WIDTH SPACE..RIGHT-TO-LEFT MARK
    (0x202A, 0x202E),   // LEFT-TO-RIGHT EMBEDDING..RIGHT-TO-LEFT OVERRIDE
    (0x2060, 0x206F),   // WORD JOINER..NOMINAL DIGIT SHAPES (2065 reserved)
    (0x3164, 0x3164),   // HANGUL FILLER
    (0xFE00, 0xFE0F),   // VARIATION SELECTOR-1..16
    (0xFEFF, 0xFEFF),   // ZERO WIDTH NO-BREAK SPACE
    (0xFFA0, 0xFFA0),   // HALFWIDTH HANGUL FILLER
    (0xFFF0, 0xFFF8),   // reserved
    (0x1BCA0, 0x1BCA3), // SHORTHAND FORMAT LETTER OVERLAP..UP STEP
    (0x1D173, 0x1D17A), // MUSICAL SYMBOL BEGIN BEAM..END PHRASE
    (0xE0000, 0xE0FFF), // the tag block, VARIATION SELECTOR-17..256, reserved
];

/// True when `c` is hidden (see the module docs): Cc but newline and tab,
/// Zl, Zp, or Default_Ignorable_Code_Point.
pub fn is_hidden(c: char) -> bool {
    if c == '\n' || c == '\t' {
        return false;
    }
    let n = u32::from(c);
    c.is_control()
        || n == 0x2028
        || n == 0x2029
        || DEFAULT_IGNORABLE
            .iter()
            .any(|(lo, hi)| (*lo..=*hi).contains(&n))
}

/// `text` with every hidden character written as `\u{XXXX}` (uppercase
/// hex, at least four digits), and how many there were.
pub fn escape_hidden(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut hidden = 0;
    for c in text.chars() {
        if is_hidden(c) {
            hidden += 1;
            out.push_str(&format!("\\u{{{:04X}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    (out, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_what_a_terminal_would_not_show() {
        for c in [
            '\0',
            '\r',
            '\u{1b}',
            '\u{7f}',
            '\u{9b}',
            '\u{AD}',
            '\u{200B}',
            '\u{200E}',
            '\u{202E}',
            '\u{2028}',
            '\u{2029}',
            '\u{2066}',
            '\u{FEFF}',
            '\u{E0041}',
            '\u{E007F}',
        ] {
            assert!(is_hidden(c), "{:04X}", u32::from(c));
        }
        for c in ['\n', '\t', ' ', 'a', 'é', '中', '\u{2014}', '\u{1F600}'] {
            assert!(!is_hidden(c), "{:04X}", u32::from(c));
        }
    }

    #[test]
    fn hidden_characters_are_written_as_escapes() {
        assert_eq!(
            escape_hidden("a\u{202E}b\u{E0041}c\n\td"),
            ("a\\u{202E}b\\u{E0041}c\n\td".to_string(), 2)
        );
        assert_eq!(escape_hidden("\u{7}"), ("\\u{0007}".to_string(), 1));
    }
}
