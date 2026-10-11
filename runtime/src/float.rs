//! The text form of floats, shared by `println`, `zore/strconv`, and `zore/math`.

/// The shortest decimal digits that read back to the value, and the exponent of the first digit.
fn shortest(value: f64, single: bool) -> (String, i32) {
    let text = if single {
        format!("{:e}", (value as f32).abs())
    } else {
        format!("{:e}", value.abs())
    };
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exponent.parse().unwrap_or(0))
}

/// `digits` rounded to `count` significant digits, with the exponent of the first digit.
fn rounded(value: f64, count: usize, single: bool) -> (String, i32) {
    let precision = count.saturating_sub(1);
    let text = if single {
        format!("{:.precision$e}", (value as f32).abs())
    } else {
        format!("{:.precision$e}", value.abs())
    };
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exponent.parse().unwrap_or(0))
}

fn special(value: f64) -> Option<&'static str> {
    if value.is_nan() {
        Some("NaN")
    } else if value == f64::INFINITY {
        Some("+Inf")
    } else if value == f64::NEG_INFINITY {
        Some("-Inf")
    } else {
        None
    }
}

fn sign(value: f64) -> &'static str {
    if value.is_sign_negative() { "-" } else { "" }
}

/// `d.ddde±XX`, with at least two exponent digits.
fn exponent_form(digits: &str, exponent: i32, upper: bool) -> String {
    let mut out = String::new();
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    }
    out.push(if upper { 'E' } else { 'e' });
    out.push(if exponent < 0 { '-' } else { '+' });
    out.push_str(&format!("{:02}", exponent.unsigned_abs()));
    out
}

/// The digits placed around a decimal point, with exactly `decimals` digits after it.
fn fixed_form(digits: &str, exponent: i32, decimals: usize) -> String {
    let point = exponent + 1;
    let mut whole = String::new();
    let mut fraction = String::new();
    if point <= 0 {
        whole.push('0');
        fraction.push_str(&"0".repeat(point.unsigned_abs() as usize));
        fraction.push_str(digits);
    } else {
        let point = point as usize;
        if digits.len() <= point {
            whole.push_str(digits);
            whole.push_str(&"0".repeat(point - digits.len()));
        } else {
            whole.push_str(&digits[..point]);
            fraction.push_str(&digits[point..]);
        }
    }
    while fraction.len() < decimals {
        fraction.push('0');
    }
    fraction.truncate(decimals);
    if decimals == 0 {
        whole
    } else {
        format!("{whole}.{fraction}")
    }
}

/// The text `println` writes: the shortest decimal that reads back to the value,
/// in exponent form below 1e-4 or from 1e21 up, and with `.0` when it is whole.
pub(super) fn default_text(value: f64, single: bool) -> String {
    if let Some(text) = special(value) {
        return text.to_string();
    }
    if value == 0.0 {
        return format!("{}0.0", sign(value));
    }
    let (digits, exponent) = shortest(value, single);
    let body = if !(-4..21).contains(&exponent) {
        exponent_form(&digits, exponent, false)
    } else {
        let decimals = (digits.len() as i32 - exponent - 1).max(1) as usize;
        fixed_form(&digits, exponent, decimals)
    };
    format!("{}{body}", sign(value))
}

/// `strconv.FormatFloat`; `None` for a format byte it does not know.
pub(super) fn format(value: f64, format: u8, precision: i64, single: bool) -> Option<String> {
    if !matches!(format, b'e' | b'E' | b'f' | b'g' | b'G') {
        return None;
    }
    if let Some(text) = special(value) {
        return Some(text.to_string());
    }
    if format == b'g' && precision < 0 {
        return Some(default_text(value, single));
    }
    let body = match format {
        b'e' | b'E' => {
            let (digits, exponent) = if value == 0.0 {
                ("0".repeat(precision.max(0) as usize + 1), 0)
            } else if precision < 0 {
                shortest(value, single)
            } else {
                rounded(value, precision as usize + 1, single)
            };
            exponent_form(&digits, exponent, format == b'E')
        }
        b'f' => {
            if precision < 0 {
                let (digits, exponent) = if value == 0.0 {
                    ("0".to_string(), 0)
                } else {
                    shortest(value, single)
                };
                let decimals = (digits.len() as i32 - exponent - 1).max(0) as usize;
                fixed_form(&digits, exponent, decimals)
            } else {
                let decimals = precision as usize;
                if single {
                    format!("{:.decimals$}", (value as f32).abs())
                } else {
                    format!("{:.decimals$}", value.abs())
                }
            }
        }
        _ => {
            let (digits, exponent) = if value == 0.0 {
                ("0".to_string(), 0)
            } else if precision < 0 {
                shortest(value, single)
            } else {
                rounded(value, precision.max(1) as usize, single)
            };
            let limit = if precision < 0 {
                21
            } else {
                precision.max(1) as i32
            };
            let digits = digits.trim_end_matches('0');
            let digits = if digits.is_empty() { "0" } else { digits };
            if exponent < -4 || exponent >= limit {
                exponent_form(digits, exponent, format == b'G')
            } else {
                let decimals = (digits.len() as i32 - exponent - 1).max(0) as usize;
                fixed_form(digits, exponent, decimals)
            }
        }
    };
    Some(format!("{}{body}", sign(value)))
}

pub(super) enum ParseFailure {
    Syntax,
    /// The value overflowed to an infinity of this sign.
    Range(f64),
}

/// `strconv.ParseFloat`: plain decimal text, `NaN`, or an infinity.
pub(super) fn parse(text: &str, single: bool) -> Result<f64, ParseFailure> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let negative = text.starts_with('-');
    let lower = unsigned.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Ok(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if lower == "nan" {
        return Ok(f64::NAN);
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.len() + fraction.len() == 0 || !digits(whole) || !digits(fraction) {
        return Err(ParseFailure::Syntax);
    }
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !digits(exponent) {
            return Err(ParseFailure::Syntax);
        }
    }
    let value = if single {
        text.parse::<f32>().map(f64::from)
    } else {
        text.parse::<f64>()
    }
    .map_err(|_| ParseFailure::Syntax)?;
    if value.is_infinite() {
        return Err(ParseFailure::Range(value));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{default_text, format, parse};

    #[test]
    fn default_text_is_the_shortest_round_trip_with_a_point() {
        for (value, text) in [
            (3.0, "3.0"),
            (0.1, "0.1"),
            (-2.5, "-2.5"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (100.0, "100.0"),
            (1e20, "100000000000000000000.0"),
            (1e21, "1e+21"),
            (1.5e300, "1.5e+300"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.0 / 3.0, "0.3333333333333333"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "+Inf"),
            (f64::NEG_INFINITY, "-Inf"),
        ] {
            assert_eq!(default_text(value, false), text, "{value:e}");
        }
        assert_eq!(default_text(f64::from(0.1f32), true), "0.1");
        assert_eq!(default_text(f64::from(f32::MAX), true), "3.4028235e+38");
    }

    #[test]
    fn explicit_formats_follow_their_precision() {
        for (value, fmt, precision, text) in [
            (2.34567, b'f', 2, "2.35"),
            (3.0, b'f', -1, "3"),
            (3.0, b'f', 0, "3"),
            (1234.5678, b'e', 3, "1.235e+03"),
            (1234.5678, b'E', -1, "1.2345678E+03"),
            (0.0, b'e', 2, "0.00e+00"),
            (1234.5678, b'g', 3, "1.23e+03"),
            (0.000012345, b'g', 2, "1.2e-05"),
            (100.0, b'g', 5, "100"),
            (2.5, b'g', -1, "2.5"),
            (-1.0, b'g', -1, "-1.0"),
            (f64::NAN, b'f', 2, "NaN"),
        ] {
            assert_eq!(format(value, fmt, precision, false).unwrap(), text);
        }
        assert!(format(1.0, b'x', -1, false).is_none());
    }

    #[test]
    fn parsing_reads_plain_decimal_text_only() {
        for (text, value) in [
            ("2.75", 2.75),
            ("-2", -2.0),
            ("1e-5", 1e-5),
            ("+.5", 0.5),
            ("7.", 7.0),
            ("1.7976931348623157e308", f64::MAX),
        ] {
            assert_eq!(parse(text, false).ok(), Some(value), "{text}");
        }
        assert!(parse("Inf", false).ok().is_some_and(f64::is_infinite));
        assert!(parse("-infinity", false).ok() == Some(f64::NEG_INFINITY));
        assert!(parse("NaN", false).ok().is_some_and(f64::is_nan));
        for text in ["", ".", "1_000", "0x1p-2", "1e", "e5", " 1", "1.2.3", "--1"] {
            assert!(parse(text, false).is_err(), "{text}");
        }
        assert!(
            matches!(parse("1e400", false), Err(super::ParseFailure::Range(v)) if v.is_infinite())
        );
        assert!(matches!(
            parse("1e39", true),
            Err(super::ParseFailure::Range(_))
        ));
    }

    #[test]
    fn default_text_reads_back_to_the_same_value() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let wide = f64::from_bits(state);
            if wide.is_finite() {
                let text = default_text(wide, false);
                assert_eq!(
                    parse(&text, false).ok().map(f64::to_bits),
                    Some(wide.to_bits()),
                    "{text}"
                );
            }
            let narrow = f32::from_bits(state as u32);
            if narrow.is_finite() {
                let text = default_text(f64::from(narrow), true);
                let back = parse(&text, true).ok().map(|v| (v as f32).to_bits());
                assert_eq!(back, Some(narrow.to_bits()), "{text}");
            }
        }
    }
}
