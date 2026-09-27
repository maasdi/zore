//! Constant evaluation (spec §6.6–6.7): untyped integer and float kinds with
//! exact arithmetic, representability, and typed constant folding.
//!
//! Implementation limits, all at or above the §6.7 minimums:
//! untyped integers up to [`MAX_INT_BITS`] bits (larger values are errors);
//! untyped floats are exact rationals, rounded to [`ROUND_PRECISION`]
//! significant bits if their size exceeds [`MAX_RATIONAL_BITS`], with a
//! binary exponent range of ±[`MAX_FLOAT_LOG2`] (overflow is an error and
//! smaller magnitudes round to zero).

use std::cmp::Ordering;

use crate::ast::{BinaryOp, UnaryOp};
use crate::bignum::{BigInt, Rational};
use crate::types::{FloatType, IntType};

pub const MAX_INT_BITS: u64 = 4096;
pub const MAX_RATIONAL_BITS: u64 = 40_000;
pub const ROUND_PRECISION: u32 = 512;
/// A 16-bit signed binary exponent, as §6.7 requires at minimum.
pub const MAX_FLOAT_LOG2: i64 = 32_767;

/// An untyped numeric constant (§6.7).
#[derive(Clone, Debug, PartialEq)]
pub enum Untyped {
    Int(BigInt),
    Float(Rational),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Folded {
    Untyped(Untyped),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstError {
    DivisionByZero,
    /// The operator needs integer operands (`%`, bitwise, shifts).
    NeedsInteger,
    /// `!`, `&&`, or `||` applied to a number.
    NeedsBool,
    /// An untyped integer beyond [`MAX_INT_BITS`].
    IntLimit,
    /// An untyped float beyond the exponent range.
    FloatOverflow,
    /// A typed result not representable in its type.
    Overflow,
}

/// Why an untyped constant cannot take a type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unrepresentable {
    NotInteger,
    OutOfRange,
}

impl Untyped {
    fn to_rational(&self) -> Rational {
        match self {
            Self::Int(v) => Rational::from_int(v.clone()),
            Self::Float(r) => r.clone(),
        }
    }

    /// The integer value of an integer, or of an integral float.
    pub fn to_integer(&self) -> Option<BigInt> {
        match self {
            Self::Int(v) => Some(v.clone()),
            Self::Float(r) => r.to_integer(),
        }
    }

    /// Short text for diagnostics.
    pub fn describe(&self) -> String {
        match self {
            Self::Int(v) => {
                let text = v.to_string();
                if text.len() > 40 {
                    format!("{}…{}", &text[..20], &text[text.len() - 10..])
                } else {
                    text
                }
            }
            Self::Float(r) => match r.to_float(crate::bignum::BINARY64) {
                Some(f) => format!("{f:?}"),
                None => "a very large value".into(),
            },
        }
    }
}

fn int(value: BigInt) -> Result<Untyped, ConstError> {
    if value.bits() > MAX_INT_BITS {
        Err(ConstError::IntLimit)
    } else {
        Ok(Untyped::Int(value))
    }
}

/// Keep an untyped float within the implementation's range and size.
pub fn float(value: Rational) -> Result<Untyped, ConstError> {
    if value.is_zero() {
        return Ok(Untyped::Float(value));
    }
    let log2 = value.floor_log2();
    if log2 >= MAX_FLOAT_LOG2 {
        return Err(ConstError::FloatOverflow);
    }
    if log2 < -MAX_FLOAT_LOG2 {
        return Ok(Untyped::Float(Rational::from_int(BigInt::zero())));
    }
    if value.size_bits() > MAX_RATIONAL_BITS {
        return Ok(Untyped::Float(value.round_to_precision(ROUND_PRECISION)));
    }
    Ok(Untyped::Float(value))
}

fn compare(op: BinaryOp, ordering: Ordering) -> bool {
    match op {
        BinaryOp::Eq => ordering.is_eq(),
        BinaryOp::NotEq => ordering.is_ne(),
        BinaryOp::Lt => ordering.is_lt(),
        BinaryOp::LtEq => ordering.is_le(),
        BinaryOp::Gt => ordering.is_gt(),
        BinaryOp::GtEq => ordering.is_ge(),
        _ => unreachable!("not a comparison"),
    }
}

/// A binary operation other than a shift on two untyped constants.
pub fn untyped_binary(op: BinaryOp, a: &Untyped, b: &Untyped) -> Result<Folded, ConstError> {
    if op.is_comparison() {
        let ordering = match (a, b) {
            (Untyped::Int(x), Untyped::Int(y)) => x.cmp(y),
            _ => a.to_rational().cmp(&b.to_rational()),
        };
        return Ok(Folded::Bool(compare(op, ordering)));
    }
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return Err(ConstError::NeedsBool);
    }
    let value = match (a, b) {
        (Untyped::Int(x), Untyped::Int(y)) => int(match op {
            BinaryOp::Add => x.add(y),
            BinaryOp::Sub => x.sub(y),
            BinaryOp::Mul => x.mul(y),
            BinaryOp::Div => x.div_rem(y).ok_or(ConstError::DivisionByZero)?.0,
            BinaryOp::Rem => x.div_rem(y).ok_or(ConstError::DivisionByZero)?.1,
            BinaryOp::BitAnd => x.and(y),
            BinaryOp::BitOr => x.or(y),
            BinaryOp::BitXor => x.xor(y),
            _ => unreachable!("shifts, comparisons, and logic are handled elsewhere"),
        })?,
        _ => {
            let (x, y) = (a.to_rational(), b.to_rational());
            float(match op {
                BinaryOp::Add => x.add(&y),
                BinaryOp::Sub => x.sub(&y),
                BinaryOp::Mul => x.mul(&y),
                BinaryOp::Div => x.div(&y).ok_or(ConstError::DivisionByZero)?,
                _ => return Err(ConstError::NeedsInteger),
            })?
        }
    };
    Ok(Folded::Untyped(value))
}

pub fn untyped_unary(op: UnaryOp, a: &Untyped) -> Result<Untyped, ConstError> {
    match (op, a) {
        (UnaryOp::Plus, _) => Ok(a.clone()),
        (UnaryOp::Neg, Untyped::Int(v)) => int(v.neg()),
        (UnaryOp::Neg, Untyped::Float(r)) => Ok(Untyped::Float(r.neg())),
        (UnaryOp::Complement, Untyped::Int(v)) => int(v.not()),
        (UnaryOp::Complement, Untyped::Float(_)) => Err(ConstError::NeedsInteger),
        (UnaryOp::Not, _) => Err(ConstError::NeedsBool),
    }
}

/// Shift an untyped constant (already known to be integral) by a count.
/// Left shifts multiply exactly; right shifts round toward negative
/// infinity (§6.6).
pub fn untyped_shift(op: BinaryOp, value: &BigInt, count: &BigInt) -> Result<Untyped, ConstError> {
    let count = count.to_u64().ok_or(ConstError::IntLimit)?;
    if op == BinaryOp::Shl {
        if value.is_zero() {
            return Ok(Untyped::Int(BigInt::zero()));
        }
        if value.bits().saturating_add(count) > MAX_INT_BITS {
            return Err(ConstError::IntLimit);
        }
        return int(value.shl(count));
    }
    Ok(Untyped::Int(value.shr_floor(count.min(MAX_INT_BITS + 1))))
}

/// Representability in an integer type (§6.7).
pub fn to_int(value: &Untyped, ty: IntType) -> Result<i128, Unrepresentable> {
    let integer = value.to_integer().ok_or(Unrepresentable::NotInteger)?;
    integer
        .to_i128()
        .filter(|&v| ty.contains(v))
        .ok_or(Unrepresentable::OutOfRange)
}

/// Representability in a float type: round to nearest, ties to even, without
/// overflow (§6.7). Returns the exact value of the rounded result.
pub fn to_float(value: &Untyped, ty: FloatType) -> Option<f64> {
    value.to_rational().to_float(ty.format())
}

/// Fold a typed integer operation; the result must fit the type (§6.6).
pub fn typed_int(op: BinaryOp, x: i128, y: i128, ty: IntType) -> Result<i128, ConstError> {
    if matches!(op, BinaryOp::Div | BinaryOp::Rem) && y == 0 {
        return Err(ConstError::DivisionByZero);
    }
    let value = match op {
        BinaryOp::Add => x.checked_add(y),
        BinaryOp::Sub => x.checked_sub(y),
        BinaryOp::Mul => x.checked_mul(y),
        BinaryOp::Div => x.checked_div(y),
        BinaryOp::Rem => x.checked_rem(y),
        BinaryOp::BitAnd => Some(ty.wrap(x & y)),
        BinaryOp::BitOr => Some(ty.wrap(x | y)),
        BinaryOp::BitXor => Some(ty.wrap(x ^ y)),
        _ => unreachable!("integer arithmetic operators"),
    };
    value
        .filter(|&v| ty.contains(v))
        .ok_or(ConstError::Overflow)
}

/// Fold a typed float operation: the exact result rounded to the type, which
/// is the correctly rounded IEEE result (§6.6). Overflow and division by zero
/// are compile-time errors for constants.
pub fn typed_float(op: BinaryOp, x: f64, y: f64, ty: FloatType) -> Result<f64, ConstError> {
    let (a, b) = (Rational::from_f64(x), Rational::from_f64(y));
    let exact = match op {
        BinaryOp::Add => a.add(&b),
        BinaryOp::Sub => a.sub(&b),
        BinaryOp::Mul => a.mul(&b),
        BinaryOp::Div => a.div(&b).ok_or(ConstError::DivisionByZero)?,
        _ => return Err(ConstError::NeedsInteger),
    };
    exact.to_float(ty.format()).ok_or(ConstError::Overflow)
}

/// Round a typed float constant to another float type (§6.6 narrowing).
pub fn float_to_float(x: f64, ty: FloatType) -> Option<f64> {
    Rational::from_f64(x).to_float(ty.format())
}

/// Convert a typed float constant to an integer type: it must be integral
/// and in range (§6.6–6.7).
pub fn float_to_int(x: f64, ty: IntType) -> Result<i128, Unrepresentable> {
    to_int(&Untyped::Float(Rational::from_f64(x)), ty)
}

/// Convert a typed integer constant to a float type.
pub fn int_to_float(x: i128, ty: FloatType) -> Option<f64> {
    Rational::from_int(BigInt::from_i128(x)).to_float(ty.format())
}

/// Parse an integer literal spelling (§3.11–3.12).
pub fn parse_int(text: &str, radix: u32) -> Result<Untyped, ConstError> {
    let digits: String = text
        .get(if radix == 10 { 0 } else { 2 }..)
        .unwrap_or("")
        .chars()
        .filter(|&c| c != '_')
        .collect();
    int(BigInt::parse(&digits, radix).expect("the lexer validated the digits"))
}

/// Parse a decimal float spelling (§3.13).
pub fn parse_float(text: &str) -> Result<Untyped, ConstError> {
    let value =
        crate::bignum::parse_decimal(text, MAX_FLOAT_LOG2).ok_or(ConstError::FloatOverflow)?;
    float(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(v: i128) -> Untyped {
        Untyped::Int(BigInt::from_i128(v))
    }

    fn f(text: &str) -> Untyped {
        parse_float(text).unwrap()
    }

    fn bin(op: BinaryOp, a: &Untyped, b: &Untyped) -> Result<Folded, ConstError> {
        untyped_binary(op, a, b)
    }

    #[test]
    fn untyped_operators_follow_go() {
        use BinaryOp::*;
        assert_eq!(bin(Add, &i(2), &f("3.0")), Ok(Folded::Untyped(f("5.0"))));
        assert_eq!(bin(Div, &i(15), &i(4)), Ok(Folded::Untyped(i(3))));
        assert_eq!(bin(Div, &i(15), &f("4.0")), Ok(Folded::Untyped(f("3.75"))));
        assert_eq!(bin(Div, &i(-7), &i(2)), Ok(Folded::Untyped(i(-3))));
        assert_eq!(bin(Rem, &i(-7), &i(3)), Ok(Folded::Untyped(i(-1))));
        assert_eq!(bin(Rem, &f("7.5"), &i(2)), Err(ConstError::NeedsInteger));
        assert_eq!(bin(Div, &i(1), &i(0)), Err(ConstError::DivisionByZero));
        assert_eq!(
            bin(Div, &f("1.0"), &f("0.0")),
            Err(ConstError::DivisionByZero)
        );
        assert_eq!(bin(Rem, &i(5), &i(0)), Err(ConstError::DivisionByZero));
        assert_eq!(bin(BitAnd, &i(6), &i(3)), Ok(Folded::Untyped(i(2))));
        assert_eq!(bin(BitOr, &i(-4), &i(1)), Ok(Folded::Untyped(i(-3))));
        assert_eq!(bin(BitAnd, &i(-4), &i(7)), Ok(Folded::Untyped(i(4))));
        assert_eq!(bin(BitAnd, &f("1.5"), &i(1)), Err(ConstError::NeedsInteger));
        assert_eq!(bin(Lt, &i(1), &f("1.5")), Ok(Folded::Bool(true)));
        assert_eq!(
            bin(Eq, &f("0.1").clone(), &f("0.1")),
            Ok(Folded::Bool(true))
        );
        assert_eq!(untyped_unary(UnaryOp::Complement, &i(1)), Ok(i(-2)));
        assert_eq!(untyped_unary(UnaryOp::Complement, &i(-1)), Ok(i(0)));
        assert_eq!(
            untyped_unary(UnaryOp::Complement, &f("1.0")),
            Err(ConstError::NeedsInteger)
        );
        // Exact decimal arithmetic: 0.1 * 3 == 0.3.
        assert_eq!(bin(Mul, &f("0.1"), &i(3)), Ok(Folded::Untyped(f("0.3"))));
    }

    #[test]
    fn shifts_and_limits() {
        let one = BigInt::from_i128(1);
        let huge = untyped_shift(BinaryOp::Shl, &one, &BigInt::from_i128(100)).unwrap();
        let Untyped::Int(huge) = huge else { panic!() };
        assert_eq!(
            untyped_shift(BinaryOp::Shr, &huge, &BigInt::from_i128(98)),
            Ok(i(4))
        );
        assert_eq!(
            untyped_shift(BinaryOp::Shr, &BigInt::from_i128(-5), &BigInt::from_i128(1)),
            Ok(i(-3))
        );
        assert!(untyped_shift(BinaryOp::Shl, &one, &BigInt::from_i128(254)).is_ok());
        assert_eq!(
            untyped_shift(BinaryOp::Shl, &one, &BigInt::from_i128(5000)),
            Err(ConstError::IntLimit)
        );
        assert_eq!(
            parse_float("1e10000").unwrap_err(),
            ConstError::FloatOverflow
        );
        assert!(parse_float("1e9000").is_ok());
    }

    #[test]
    fn representability() {
        let int8 = IntType {
            bits: 8,
            signed: false,
        };
        let f32t = FloatType { bits: 32 };
        let f64t = FloatType { bits: 64 };
        assert_eq!(to_int(&f("42.0"), int8), Ok(42));
        assert_eq!(to_int(&f("1.1"), int8), Err(Unrepresentable::NotInteger));
        assert_eq!(to_int(&i(1024), int8), Err(Unrepresentable::OutOfRange));
        assert_eq!(
            to_int(
                &f("1e10"),
                IntType {
                    bits: 64,
                    signed: false
                }
            ),
            Ok(10_000_000_000)
        );
        assert_eq!(to_float(&f("0.1"), f32t), Some(f64::from(0.1f32)));
        assert_eq!(to_float(&f("1e1000"), f64t), None);
        assert_eq!(to_float(&f("1e39"), f32t), None);
        assert_eq!(to_float(&f("1e-1000"), f64t), Some(0.0));
        assert_eq!(to_float(&i(1), f64t), Some(1.0));
        assert_eq!(typed_float(BinaryOp::Mul, 0.1, 3.0, f64t), Ok(0.1 * 3.0));
        assert_eq!(
            typed_float(BinaryOp::Add, f64::from(0.1f32), f64::from(0.2f32), f32t),
            Ok(f64::from(0.1f32 + 0.2f32))
        );
        assert_eq!(
            typed_float(BinaryOp::Div, 1.0, 0.0, f64t),
            Err(ConstError::DivisionByZero)
        );
        assert_eq!(
            typed_float(BinaryOp::Mul, f64::MAX, 2.0, f64t),
            Err(ConstError::Overflow)
        );
        assert_eq!(
            float_to_int(
                2.0,
                IntType {
                    bits: 64,
                    signed: true
                }
            ),
            Ok(2)
        );
        assert_eq!(
            float_to_int(
                2.5,
                IntType {
                    bits: 64,
                    signed: true
                }
            ),
            Err(Unrepresentable::NotInteger)
        );
        assert_eq!(float_to_float(1e300, f32t), None);
        assert_eq!(
            int_to_float(i128::from(u64::MAX), f32t),
            Some(f64::from(u64::MAX as f32))
        );
    }
}
