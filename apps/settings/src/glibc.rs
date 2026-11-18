//! Math this program brings itself, so that it starts on an older glibc.
//!
//! glibc 2.43 and 2.44 gave these functions new symbol versions. A program
//! linked on such a system asks for exactly those, and does not start where
//! glibc is older: Ubuntu 24.04 has 2.39. Defined here, the linker never
//! asks glibc for them. Denial packages Settings for those systems and
//! checks the program against its glibc baseline.
//!
//! This is every function of glibc 2.39 that 2.44 names by a newer version,
//! used here or not: which of them a build asks for changes with the
//! compiler and the profile. `objdump -T /usr/lib/libm.so.6` lists them; a
//! later glibc may add to them, and a function Denial's check names then
//! belongs here.

use std::ffi::c_int;

macro_rules! own {
    ($($name:ident($($argument:ident),+) -> $number:ty;)+) => {$(
        #[unsafe(no_mangle)]
        extern "C" fn $name($($argument: $number),+) -> $number {
            libm::$name($($argument),+)
        }
    )+};
}

own! {
    acosf(x) -> f32;
    acoshf(x) -> f32;
    asinf(x) -> f32;
    atan2f(y, x) -> f32;
    atanhf(x) -> f32;
    coshf(x) -> f32;
    sinhf(x) -> f32;
    lgammaf(x) -> f32;
    log10f(x) -> f32;
    tgammaf(x) -> f32;
    j0f(x) -> f32;
    j1f(x) -> f32;
    y0f(x) -> f32;
    y1f(x) -> f32;
    remainderf(x, y) -> f32;
    sqrtf(x) -> f32;
    cosh(x) -> f64;
    sinh(x) -> f64;
    remainder(x, y) -> f64;
}

#[unsafe(no_mangle)]
extern "C" fn jnf(order: c_int, x: f32) -> f32 {
    libm::jnf(order, x)
}

#[unsafe(no_mangle)]
extern "C" fn ynf(order: c_int, x: f32) -> f32 {
    libm::ynf(order, x)
}

/// # Safety
/// `sign` points to an `int`, as C's `lgammaf_r` takes it.
#[unsafe(no_mangle)]
unsafe extern "C" fn lgammaf_r(x: f32, sign: *mut c_int) -> f32 {
    let (value, of) = libm::lgammaf_r(x);
    // SAFETY: the caller's `int`, by this function's contract.
    unsafe { *sign = of };
    value
}
