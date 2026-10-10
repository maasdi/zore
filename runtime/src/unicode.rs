//! The code behind `zore/unicode`.

/// Inclusive ranges of general category Nd, from Unicode 15.1.0.
const DECIMAL_DIGITS: &[(u32, u32)] = &[
    (0x30, 0x39),
    (0x660, 0x669),
    (0x6F0, 0x6F9),
    (0x7C0, 0x7C9),
    (0x966, 0x96F),
    (0x9E6, 0x9EF),
    (0xA66, 0xA6F),
    (0xAE6, 0xAEF),
    (0xB66, 0xB6F),
    (0xBE6, 0xBEF),
    (0xC66, 0xC6F),
    (0xCE6, 0xCEF),
    (0xD66, 0xD6F),
    (0xDE6, 0xDEF),
    (0xE50, 0xE59),
    (0xED0, 0xED9),
    (0xF20, 0xF29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10D30, 0x10D39),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E950, 0x1E959),
    (0x1FBF0, 0x1FBF9),
];

fn scalar(rune: u32) -> char {
    char::from_u32(rune).unwrap_or(char::REPLACEMENT_CHARACTER)
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_letter(rune: u32) -> bool {
    scalar(rune).is_alphabetic()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_digit(rune: u32) -> bool {
    DECIMAL_DIGITS
        .binary_search_by(|&(low, high)| {
            if high < rune {
                std::cmp::Ordering::Less
            } else if low > rune {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_number(rune: u32) -> bool {
    scalar(rune).is_numeric()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_space(rune: u32) -> bool {
    scalar(rune).is_whitespace()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_upper(rune: u32) -> bool {
    scalar(rune).is_uppercase()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_lower(rune: u32) -> bool {
    scalar(rune).is_lowercase()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_is_control(rune: u32) -> bool {
    scalar(rune).is_control()
}

fn single(mut mapped: impl Iterator<Item = char>, rune: u32) -> u32 {
    match (mapped.next(), mapped.next()) {
        (Some(one), None) => one as u32,
        _ => rune,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_to_upper(rune: u32) -> u32 {
    single(scalar(rune).to_uppercase(), rune)
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_unicode_to_lower(rune: u32) -> u32 {
    single(scalar(rune).to_lowercase(), rune)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_mappings() {
        assert!(zore_native_unicode_is_digit('7' as u32));
        assert!(zore_native_unicode_is_digit('٣' as u32));
        assert!(!zore_native_unicode_is_digit('½' as u32));
        assert!(zore_native_unicode_is_number('½' as u32));
        assert!(zore_native_unicode_is_letter('é' as u32));
        assert!(!zore_native_unicode_is_letter('1' as u32));
        assert!(zore_native_unicode_is_space('\u{3000}' as u32));
        assert_eq!(zore_native_unicode_to_upper('é' as u32), 'É' as u32);
        assert_eq!(zore_native_unicode_to_upper('ß' as u32), 'ß' as u32);
        assert_eq!(zore_native_unicode_to_lower('Σ' as u32), 'σ' as u32);
        assert!(DECIMAL_DIGITS.windows(2).all(|pair| pair[0].1 < pair[1].0));
    }
}
