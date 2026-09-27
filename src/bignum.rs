//! Arbitrary-precision integers and exact rationals for constant evaluation
//! (spec §6.7).
//!
//! Hand-written to keep the compiler free of numeric dependencies and easy to
//! port. Performance targets compile-time constants, not general computation:
//! multiplication is schoolbook and multi-limb division is bitwise.

use std::cmp::Ordering;
use std::fmt;

// ----- magnitude helpers: little-endian u32 limbs without trailing zeros -----

fn trim(mut v: Vec<u32>) -> Vec<u32> {
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

fn mag_cmp(a: &[u32], b: &[u32]) -> Ordering {
    a.len()
        .cmp(&b.len())
        .then_with(|| a.iter().rev().cmp(b.iter().rev()))
}

fn mag_add(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for (i, &limb) in long.iter().enumerate() {
        let sum = u64::from(limb) + u64::from(short.get(i).copied().unwrap_or(0)) + carry;
        out.push(sum as u32);
        carry = sum >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}

/// `a - b` for `a >= b`.
fn mag_sub(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (i, &limb) in a.iter().enumerate() {
        let mut diff = i64::from(limb) - i64::from(b.get(i).copied().unwrap_or(0)) - borrow;
        borrow = 0;
        if diff < 0 {
            diff += 1 << 32;
            borrow = 1;
        }
        out.push(diff as u32);
    }
    debug_assert_eq!(borrow, 0, "mag_sub requires a >= b");
    trim(out)
}

fn mag_mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let t = u64::from(x) * u64::from(y) + u64::from(out[i + j]) + carry;
            out[i + j] = t as u32;
            carry = t >> 32;
        }
        out[i + b.len()] = carry as u32;
    }
    trim(out)
}

fn mag_bits(a: &[u32]) -> u64 {
    match a.last() {
        None => 0,
        Some(&top) => (a.len() as u64 - 1) * 32 + u64::from(32 - top.leading_zeros()),
    }
}

fn mag_bit(a: &[u32], bit: u64) -> bool {
    a.get((bit / 32) as usize)
        .is_some_and(|limb| limb >> (bit % 32) & 1 == 1)
}

fn mag_shl(a: &[u32], n: u64) -> Vec<u32> {
    if a.is_empty() {
        return Vec::new();
    }
    let (limbs, bits) = ((n / 32) as usize, (n % 32) as u32);
    let mut out = vec![0u32; limbs];
    if bits == 0 {
        out.extend_from_slice(a);
    } else {
        let mut carry = 0u32;
        for &limb in a {
            out.push(limb << bits | carry);
            carry = limb >> (32 - bits);
        }
        out.push(carry);
    }
    trim(out)
}

fn mag_shr(a: &[u32], n: u64) -> Vec<u32> {
    let limbs = (n / 32) as usize;
    if limbs >= a.len() {
        return Vec::new();
    }
    let bits = (n % 32) as u32;
    let src = &a[limbs..];
    let out = if bits == 0 {
        src.to_vec()
    } else {
        (0..src.len())
            .map(|i| src[i] >> bits | src.get(i + 1).map_or(0, |&hi| hi << (32 - bits)))
            .collect()
    };
    trim(out)
}

/// Whether any of the low `n` bits are set.
fn mag_low_bits_nonzero(a: &[u32], n: u64) -> bool {
    let limbs = (n / 32) as usize;
    let bits = (n % 32) as u32;
    a.iter().take(limbs).any(|&l| l != 0)
        || (bits != 0 && a.get(limbs).is_some_and(|&l| l & ((1u32 << bits) - 1) != 0))
}

fn mag_divrem_small(a: &[u32], d: u32) -> (Vec<u32>, u32) {
    let mut out = vec![0u32; a.len()];
    let mut rem = 0u64;
    for i in (0..a.len()).rev() {
        let cur = rem << 32 | u64::from(a[i]);
        out[i] = (cur / u64::from(d)) as u32;
        rem = cur % u64::from(d);
    }
    (trim(out), rem as u32)
}

/// Truncated division of magnitudes; `b` must be nonzero.
fn mag_divrem(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    assert!(!b.is_empty(), "division by zero");
    if mag_cmp(a, b) == Ordering::Less {
        return (Vec::new(), a.to_vec());
    }
    if b.len() == 1 {
        let (q, r) = mag_divrem_small(a, b[0]);
        return (q, trim(vec![r]));
    }
    // Restoring binary long division.
    let bits = mag_bits(a);
    let mut quotient = vec![0u32; a.len()];
    let mut rem: Vec<u32> = Vec::with_capacity(b.len() + 1);
    for bit in (0..bits).rev() {
        rem = mag_shl(&rem, 1);
        if mag_bit(a, bit) {
            if rem.is_empty() {
                rem.push(1);
            } else {
                rem[0] |= 1;
            }
        }
        if mag_cmp(&rem, b) != Ordering::Less {
            rem = mag_sub(&rem, b);
            quotient[(bit / 32) as usize] |= 1 << (bit % 32);
        }
    }
    (trim(quotient), rem)
}

fn trailing_zeros(a: &[u32]) -> u64 {
    let mut count = 0;
    for &limb in a {
        if limb == 0 {
            count += 32;
        } else {
            return count + u64::from(limb.trailing_zeros());
        }
    }
    count
}

/// Binary GCD of magnitudes.
fn mag_gcd(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let shift = trailing_zeros(a).min(trailing_zeros(b));
    let mut a = mag_shr(a, trailing_zeros(a));
    let mut b = b.to_vec();
    while !b.is_empty() {
        b = mag_shr(&b, trailing_zeros(&b));
        if mag_cmp(&a, &b) == Ordering::Greater {
            std::mem::swap(&mut a, &mut b);
        }
        b = mag_sub(&b, &a);
    }
    mag_shl(&a, shift)
}

// ----- signed integers -----

/// An arbitrary-precision signed integer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigInt {
    negative: bool,
    mag: Vec<u32>,
}

impl BigInt {
    pub fn zero() -> Self {
        Self::from_mag(false, Vec::new())
    }

    fn from_mag(negative: bool, mag: Vec<u32>) -> Self {
        let mag = trim(mag);
        Self {
            negative: negative && !mag.is_empty(),
            mag,
        }
    }

    pub fn from_i128(value: i128) -> Self {
        let mut m = value.unsigned_abs();
        let mut mag = Vec::new();
        while m != 0 {
            mag.push(m as u32);
            m >>= 32;
        }
        Self::from_mag(value < 0, mag)
    }

    pub fn from_u32(value: u32) -> Self {
        Self::from_mag(false, vec![value])
    }

    /// Parse digits valid for `radix` (2..=16), without sign or separators.
    pub fn parse(digits: &str, radix: u32) -> Option<Self> {
        let mut mag: Vec<u32> = Vec::new();
        for ch in digits.chars() {
            let digit = ch.to_digit(radix)?;
            let mut carry = u64::from(digit);
            for limb in &mut mag {
                let t = u64::from(*limb) * u64::from(radix) + carry;
                *limb = t as u32;
                carry = t >> 32;
            }
            if carry != 0 {
                mag.push(carry as u32);
            }
        }
        (!digits.is_empty()).then(|| Self::from_mag(false, mag))
    }

    pub fn is_zero(&self) -> bool {
        self.mag.is_empty()
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// Bit length of the magnitude.
    pub fn bits(&self) -> u64 {
        mag_bits(&self.mag)
    }

    pub fn to_i128(&self) -> Option<i128> {
        if self.bits() > 128 {
            return None;
        }
        let mut m: u128 = 0;
        for &limb in self.mag.iter().rev() {
            m = m << 32 | u128::from(limb);
        }
        if self.negative {
            if m <= i128::MIN.unsigned_abs() {
                Some((m as i128).wrapping_neg())
            } else {
                None
            }
        } else {
            i128::try_from(m).ok()
        }
    }

    pub fn to_u64(&self) -> Option<u64> {
        if self.negative || self.bits() > 64 {
            return None;
        }
        Some(
            self.mag
                .iter()
                .rev()
                .fold(0u64, |acc, &l| acc << 32 | u64::from(l)),
        )
    }

    pub fn neg(&self) -> Self {
        Self::from_mag(!self.negative, self.mag.clone())
    }

    pub fn abs(&self) -> Self {
        Self::from_mag(false, self.mag.clone())
    }

    pub fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            return Self::from_mag(self.negative, mag_add(&self.mag, &other.mag));
        }
        match mag_cmp(&self.mag, &other.mag) {
            Ordering::Equal => Self::zero(),
            Ordering::Greater => Self::from_mag(self.negative, mag_sub(&self.mag, &other.mag)),
            Ordering::Less => Self::from_mag(other.negative, mag_sub(&other.mag, &self.mag)),
        }
    }

    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self::from_mag(
            self.negative != other.negative,
            mag_mul(&self.mag, &other.mag),
        )
    }

    /// Quotient truncated toward zero and remainder with the dividend's sign;
    /// `None` for a zero divisor.
    pub fn div_rem(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() {
            return None;
        }
        let (q, r) = mag_divrem(&self.mag, &other.mag);
        Some((
            Self::from_mag(self.negative != other.negative, q),
            Self::from_mag(self.negative, r),
        ))
    }

    pub fn shl(&self, n: u64) -> Self {
        Self::from_mag(self.negative, mag_shl(&self.mag, n))
    }

    /// Right shift rounding toward negative infinity.
    pub fn shr_floor(&self, n: u64) -> Self {
        let q = mag_shr(&self.mag, n);
        if self.negative && mag_low_bits_nonzero(&self.mag, n) {
            Self::from_mag(true, mag_add(&q, &[1]))
        } else {
            Self::from_mag(self.negative, q)
        }
    }

    /// Two's-complement limbs of width `limbs`, which must exceed the magnitude.
    fn twos(&self, limbs: usize) -> Vec<u32> {
        let mut out = self.mag.clone();
        out.resize(limbs, 0);
        if self.negative {
            for limb in &mut out {
                *limb = !*limb;
            }
            let mut carry = 1u64;
            for limb in &mut out {
                let t = u64::from(*limb) + carry;
                *limb = t as u32;
                carry = t >> 32;
            }
        }
        out
    }

    fn from_twos(mut limbs: Vec<u32>) -> Self {
        let negative = limbs.last().is_some_and(|&l| l >> 31 == 1);
        if negative {
            for limb in &mut limbs {
                *limb = !*limb;
            }
            let magnitude = mag_add(&trim(limbs), &[1]);
            return Self::from_mag(true, magnitude);
        }
        Self::from_mag(false, limbs)
    }

    /// Bitwise operation with infinite-precision two's complement (§6.7).
    fn bitwise(&self, other: &Self, op: impl Fn(u32, u32) -> u32) -> Self {
        let limbs = self.mag.len().max(other.mag.len()) + 1;
        let (a, b) = (self.twos(limbs), other.twos(limbs));
        Self::from_twos(a.iter().zip(&b).map(|(&x, &y)| op(x, y)).collect())
    }

    pub fn and(&self, other: &Self) -> Self {
        self.bitwise(other, |x, y| x & y)
    }

    pub fn or(&self, other: &Self) -> Self {
        self.bitwise(other, |x, y| x | y)
    }

    pub fn xor(&self, other: &Self) -> Self {
        self.bitwise(other, |x, y| x ^ y)
    }

    /// Unary `^`: `-x - 1` (§6.7).
    pub fn not(&self) -> Self {
        self.neg().sub(&Self::from_u32(1))
    }

    pub fn pow(base: u32, exp: u64) -> Self {
        let mut result = Self::from_u32(1);
        let mut square = Self::from_u32(base);
        let mut exp = exp;
        while exp != 0 {
            if exp & 1 == 1 {
                result = result.mul(&square);
            }
            exp >>= 1;
            if exp != 0 {
                square = square.mul(&square);
            }
        }
        result
    }

    fn gcd(&self, other: &Self) -> Self {
        Self::from_mag(false, mag_gcd(&self.mag, &other.mag))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => mag_cmp(&self.mag, &other.mag),
            (true, true) => mag_cmp(&other.mag, &self.mag),
        }
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        let mut chunks = Vec::new();
        let mut mag = self.mag.clone();
        while !mag.is_empty() {
            let (q, r) = mag_divrem_small(&mag, 1_000_000_000);
            chunks.push(r);
            mag = q;
        }
        if self.negative {
            f.write_str("-")?;
        }
        write!(f, "{}", chunks.pop().expect("nonzero"))?;
        for chunk in chunks.iter().rev() {
            write!(f, "{chunk:09}")?;
        }
        Ok(())
    }
}

// ----- rationals -----

/// An exact rational `num / den` in lowest terms with `den > 0`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rational {
    num: BigInt,
    den: BigInt,
}

/// IEEE 754 binary formats: precision in bits and normal exponent range.
#[derive(Clone, Copy, Debug)]
pub struct FloatFormat {
    pub precision: u32,
    pub min_exp: i64,
    pub max_exp: i64,
}

pub const BINARY32: FloatFormat = FloatFormat {
    precision: 24,
    min_exp: -126,
    max_exp: 127,
};

pub const BINARY64: FloatFormat = FloatFormat {
    precision: 53,
    min_exp: -1022,
    max_exp: 1023,
};

/// `2^exp` as an f64, for exponents in f64's finite range.
fn pow2(exp: i64) -> f64 {
    if exp >= -1022 {
        f64::from_bits(((exp + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (exp + 1074))
    }
}

impl Rational {
    pub fn from_int(value: BigInt) -> Self {
        Self {
            num: value,
            den: BigInt::from_u32(1),
        }
    }

    /// `num / den`; `None` if `den` is zero.
    pub fn new(num: BigInt, den: BigInt) -> Option<Self> {
        if den.is_zero() {
            return None;
        }
        let (num, den) = if den.is_negative() {
            (num.neg(), den.neg())
        } else {
            (num, den)
        };
        let g = num.gcd(&den);
        if g == BigInt::from_u32(1) || num.is_zero() {
            let den = if num.is_zero() {
                BigInt::from_u32(1)
            } else {
                den
            };
            return Some(Self { num, den });
        }
        let num = num.div_rem(&g)?.0;
        let den = den.div_rem(&g)?.0;
        Some(Self { num, den })
    }

    /// The exact value of a finite f64.
    pub fn from_f64(value: f64) -> Self {
        assert!(value.is_finite(), "constants are finite");
        let bits = value.to_bits();
        let negative = bits >> 63 == 1;
        let exp_bits = ((bits >> 52) & 0x7ff) as i64;
        let fraction = bits & ((1 << 52) - 1);
        let (mantissa, exp) = if exp_bits == 0 {
            (fraction, -1074)
        } else {
            (fraction | 1 << 52, exp_bits - 1075)
        };
        let m = BigInt::from_i128(i128::from(mantissa));
        let m = if negative { m.neg() } else { m };
        if exp >= 0 {
            Self::from_int(m.shl(exp as u64))
        } else {
            Self::new(m, BigInt::from_u32(1).shl((-exp) as u64)).expect("nonzero denominator")
        }
    }

    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.num.is_negative()
    }

    /// The integer value, if the rational has no fractional part.
    pub fn to_integer(&self) -> Option<BigInt> {
        (self.den == BigInt::from_u32(1)).then(|| self.num.clone())
    }

    /// Bit sizes of the numerator and denominator, for precision limits.
    pub fn size_bits(&self) -> u64 {
        self.num.bits().max(self.den.bits())
    }

    pub fn neg(&self) -> Self {
        Self {
            num: self.num.neg(),
            den: self.den.clone(),
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        let num = self.num.mul(&other.den).add(&other.num.mul(&self.den));
        Self::new(num, self.den.mul(&other.den)).expect("nonzero denominator")
    }

    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self::new(self.num.mul(&other.num), self.den.mul(&other.den)).expect("nonzero denominator")
    }

    /// `None` for division by zero.
    pub fn div(&self, other: &Self) -> Option<Self> {
        Self::new(self.num.mul(&other.den), self.den.mul(&other.num))
    }

    /// `E` with `2^E <= |self| < 2^(E+1)`; `self` must be nonzero.
    pub fn floor_log2(&self) -> i64 {
        let (p, q) = (self.num.abs(), &self.den);
        let mut e = p.bits() as i64 - q.bits() as i64;
        let below = if e >= 0 {
            p < q.shl(e as u64)
        } else {
            p.shl((-e) as u64) < *q
        };
        if below {
            e -= 1;
        }
        e
    }

    /// Round `|self| * 2^shift` to the nearest integer, ties to even.
    fn round_scaled(&self, shift: i64) -> BigInt {
        let p = self.num.abs();
        let (dividend, divisor) = if shift >= 0 {
            (p.shl(shift as u64), self.den.clone())
        } else {
            (p, self.den.shl((-shift) as u64))
        };
        let (q, r) = dividend.div_rem(&divisor).expect("nonzero denominator");
        match r.shl(1).cmp(&divisor) {
            Ordering::Less => q,
            Ordering::Greater => q.add(&BigInt::from_u32(1)),
            Ordering::Equal if q.mag.first().is_some_and(|l| l & 1 == 1) => {
                q.add(&BigInt::from_u32(1))
            }
            Ordering::Equal => q,
        }
    }

    /// Round to an IEEE 754 format with round-to-nearest, ties-to-even,
    /// including subnormals. Returns `None` on overflow. Negative zero is
    /// returned as positive zero (§6.7).
    pub fn to_float(&self, format: FloatFormat) -> Option<f64> {
        if self.is_zero() {
            return Some(0.0);
        }
        let e = self.floor_log2();
        if e > format.max_exp {
            return None;
        }
        // Exponent of the least significant retained bit; below the normal
        // range it stays at the subnormal minimum.
        let lsb = e.max(format.min_exp) - (i64::from(format.precision) - 1);
        let m = self.round_scaled(-lsb);
        let m = m.to_u64().expect("rounded mantissa fits");
        let magnitude = m as f64 * pow2(lsb);
        let limit = pow2(format.max_exp) * 2.0;
        if magnitude.is_infinite() || magnitude >= limit {
            return None;
        }
        if magnitude == 0.0 {
            return Some(0.0);
        }
        Some(if self.is_negative() {
            -magnitude
        } else {
            magnitude
        })
    }

    /// Round to `precision` significant bits, for keeping constant sizes
    /// bounded (§6.7 permits rounding at the implementation's precision).
    pub fn round_to_precision(&self, precision: u32) -> Self {
        if self.is_zero() {
            return self.clone();
        }
        let lsb = self.floor_log2() - (i64::from(precision) - 1);
        let m = self.round_scaled(-lsb);
        let m = if self.is_negative() { m.neg() } else { m };
        if lsb >= 0 {
            Self::from_int(m.shl(lsb as u64))
        } else {
            Self::new(m, BigInt::from_u32(1).shl((-lsb) as u64)).expect("nonzero denominator")
        }
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        self.num.mul(&other.den).cmp(&other.num.mul(&self.den))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse a validated decimal float spelling (§3.13) into an exact rational.
/// Returns `None` if the magnitude reaches `2^max_log2`; values far below
/// `2^-max_log2` round to zero.
pub fn parse_decimal(text: &str, max_log2: i64) -> Option<Rational> {
    let text: String = text.chars().filter(|&c| c != '_').collect();
    let (significand, exponent) = match text.find(['e', 'E']) {
        Some(i) => {
            let exp = &text[i + 1..];
            let fallback = if exp.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            };
            (&text[..i], exp.parse::<i64>().unwrap_or(fallback))
        }
        None => (text.as_str(), 0),
    };
    let (whole, fraction) = significand.split_once('.').unwrap_or((significand, ""));
    let digits = format!("{whole}{fraction}");
    let mantissa = BigInt::parse(&digits, 10).expect("validated digits");
    if mantissa.is_zero() {
        return Some(Rational::from_int(mantissa));
    }
    let scale = exponent.saturating_sub(fraction.len() as i64);
    // log2(10) < 3.33; bound the work before computing powers of ten.
    let significant = digits.trim_start_matches('0').len() as i64;
    let approx_log2 = (significant.saturating_add(scale)).saturating_mul(10) / 3;
    if approx_log2 > max_log2 + 64 {
        return None;
    }
    if approx_log2 < -max_log2 - 64 {
        return Some(Rational::from_int(BigInt::zero()));
    }
    let power = BigInt::pow(10, scale.unsigned_abs());
    let value = if scale >= 0 {
        Rational::from_int(mantissa.mul(&power))
    } else {
        Rational::new(mantissa, power).expect("nonzero power")
    };
    (value.floor_log2() < max_log2).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(v: i128) -> BigInt {
        BigInt::from_i128(v)
    }

    /// Deterministic pseudo-random i128 values spanning many magnitudes.
    fn samples() -> Vec<i128> {
        let mut out = vec![
            0,
            1,
            -1,
            2,
            -2,
            i128::from(u32::MAX),
            -i128::from(u32::MAX),
            1 << 32,
        ];
        let mut state = 0x1234_5678_9abc_def1_u64;
        for _ in 0..400 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let bits = state % 100;
            let v = (i128::from(state) << 40 ^ i128::from(state.rotate_left(17))) >> (100 - bits);
            out.push(if state & 1 == 0 { v } else { -v });
        }
        out
    }

    #[test]
    fn integer_arithmetic_matches_i128() {
        let values = samples();
        for &a in values.iter().take(120) {
            for &b in values.iter().rev().take(120) {
                assert_eq!(big(a).add(&big(b)).to_i128(), a.checked_add(b), "{a} + {b}");
                assert_eq!(big(a).sub(&big(b)).to_i128(), a.checked_sub(b), "{a} - {b}");
                assert_eq!(big(a).mul(&big(b)).to_i128(), a.checked_mul(b), "{a} * {b}");
                assert_eq!(big(a).cmp(&big(b)), a.cmp(&b), "{a} cmp {b}");
                assert_eq!(big(a).and(&big(b)).to_i128(), Some(a & b), "{a} & {b}");
                assert_eq!(big(a).or(&big(b)).to_i128(), Some(a | b), "{a} | {b}");
                assert_eq!(big(a).xor(&big(b)).to_i128(), Some(a ^ b), "{a} ^ {b}");
                if b != 0 {
                    let (q, r) = big(a).div_rem(&big(b)).unwrap();
                    assert_eq!(
                        (q.to_i128(), r.to_i128()),
                        (Some(a / b), Some(a % b)),
                        "{a} / {b}"
                    );
                }
            }
            assert_eq!(big(a).not().to_i128(), Some(!a));
            assert_eq!(big(a).to_string(), a.to_string());
            for n in [0, 1, 5, 31, 32, 33, 64, 100] {
                assert_eq!(
                    big(a).shr_floor(n).to_i128(),
                    Some(a >> n.min(127)),
                    "{a} >> {n}"
                );
                if (a.unsigned_abs().leading_zeros() as u64) > n + 1 {
                    assert_eq!(big(a).shl(n).to_i128(), Some(a << n), "{a} << {n}");
                }
            }
        }
        assert!(big(0).div_rem(&big(0)).is_none());
    }

    #[test]
    fn large_values_and_parsing() {
        let huge = BigInt::from_u32(1).shl(100);
        assert_eq!(huge.to_string(), "1267650600228229401496703205376");
        assert_eq!(huge.shr_floor(98).to_i128(), Some(4));
        assert_eq!(BigInt::parse("ff", 16).unwrap().to_i128(), Some(255));
        assert_eq!(BigInt::parse("1010", 2).unwrap().to_i128(), Some(10));
        assert!(BigInt::parse("12a", 10).is_none());
        let n = BigInt::pow(10, 40);
        assert_eq!(n.to_string(), format!("1{}", "0".repeat(40)));
        let (q, r) = n.div_rem(&BigInt::pow(10, 25).add(&big(7))).unwrap();
        assert_eq!(q.mul(&BigInt::pow(10, 25).add(&big(7))).add(&r), n);
        assert!(r < BigInt::pow(10, 25).add(&big(7)));
        assert_eq!(big(-4).or(&big(1)).to_i128(), Some(-3));
        assert_eq!(big(-1).not().to_i128(), Some(0));
        assert_eq!(BigInt::from_u32(1).shl(254).bits(), 255);
    }

    fn decimal(text: &str) -> Rational {
        parse_decimal(text, 32767).unwrap()
    }

    #[test]
    fn decimal_rounding_matches_the_standard_library() {
        let mut cases: Vec<String> = [
            "0.1",
            "0.2",
            "0.3",
            "1.0",
            "2.718281828459045",
            "3.4028234663852886e38",
            "3.4028235677973366e38",
            "1.7976931348623157e308",
            "1.7976931348623158e308",
            "4.9406564584124654e-324",
            "2.4703282292062327e-324",
            "2.4703282292062328e-324",
            "1e-400",
            "1.401298464324817e-45",
            "7.006492321624085e-46",
            "7.006492321624086e-46",
            "1.1754943508222875e-38",
            "2.2250738585072014e-308",
            "9007199254740993",
            "9007199254740995",
            "16777217",
            "123456789012345678901234567890e-10",
            "0.5",
            "1e23",
            "8.589973e9",
            "1e-45",
            "0.000001",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut state = 0x9e37_79b9_u64;
        for _ in 0..3000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mantissa = state % 10_000_000_000_000_000;
            let exp = (state >> 50) as i64 % 700 - 350;
            cases.push(format!("{mantissa}e{exp}"));
            cases.push(format!("{}.{}e{}", state % 1000, state % 97, exp / 10));
        }
        for text in &cases {
            let value = decimal(text);
            let expected64 = text.parse::<f64>().unwrap();
            match value.to_float(BINARY64) {
                Some(actual) => assert_eq!(actual.to_bits(), expected64.to_bits(), "{text} f64"),
                None => assert!(expected64.is_infinite(), "{text} f64 overflow"),
            }
            let expected32 = text.parse::<f32>().unwrap();
            match value.to_float(BINARY32) {
                Some(actual) => {
                    assert_eq!(
                        (actual as f32).to_bits(),
                        expected32.to_bits(),
                        "{text} f32"
                    );
                    assert_eq!(actual as f32 as f64, actual, "{text} exact in f32");
                }
                None => assert!(expected32.is_infinite(), "{text} f32 overflow"),
            }
            let negative = value.neg();
            if let Some(actual) = negative.to_float(BINARY64) {
                let expected = -expected64;
                if expected == 0.0 {
                    assert_eq!(actual.to_bits(), 0, "negative zero becomes positive zero");
                } else {
                    assert_eq!(actual.to_bits(), expected.to_bits(), "-{text}");
                }
            }
        }
    }

    #[test]
    fn rationals_are_exact() {
        let tenth = decimal("0.1");
        assert_eq!(tenth.mul(&Rational::from_int(big(3))), decimal("0.3"));
        let c = Rational::from_int(big(15)).div(&decimal("4.0")).unwrap();
        assert_eq!(c, decimal("3.75"));
        assert_eq!(decimal("42.0").to_integer(), Some(big(42)));
        assert_eq!(decimal("1.1").to_integer(), None);
        assert!(
            Rational::from_int(big(1))
                .div(&Rational::from_int(big(0)))
                .is_none()
        );
        for value in [0.1, -2.5, 1e-310, f64::MAX, f64::MIN_POSITIVE] {
            assert_eq!(Rational::from_f64(value).to_float(BINARY64), Some(value));
        }
        assert_eq!(decimal("1e1000").to_float(BINARY64), None);
        assert!(parse_decimal("1e99999", 32767).is_none());
        assert!(parse_decimal("1e999999999999999999999", 32767).is_none());
        let tiny = parse_decimal("1e-999999999999999999999", 32767).unwrap();
        assert!(tiny.is_zero());
        assert_eq!(
            parse_decimal("1e-99999", 32767).unwrap().to_float(BINARY64),
            Some(0.0)
        );
        assert_eq!(decimal("1e9000").floor_log2(), 29897);
        let rounded = decimal("0.1").round_to_precision(8);
        assert_eq!(rounded, Rational::new(big(205), big(2048)).unwrap());
        assert_eq!(decimal("1.5").neg().floor_log2(), 0);
        assert_eq!(decimal("0.75").floor_log2(), -1);
    }
}
