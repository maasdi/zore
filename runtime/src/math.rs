//! The code behind `zore/math`.

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_floor(x: f64) -> f64 {
    x.floor()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_ceil(x: f64) -> f64 {
    x.ceil()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_trunc(x: f64) -> f64 {
    x.trunc()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_round(x: f64) -> f64 {
    x.round()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_mod(x: f64, y: f64) -> f64 {
    x % y
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_exp(x: f64) -> f64 {
    x.exp()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_log(x: f64) -> f64 {
    x.ln()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_inf(sign: i64) -> f64 {
    if sign >= 0 {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_math_na_n() -> f64 {
    f64::NAN
}
