//! Math this program brings itself, so that it starts on an older glibc.
//!
//! glibc 2.43 and 2.44 gave these functions new symbol versions. A program
//! linked on such a system asks for exactly those, and does not start where
//! glibc is older: Ubuntu 24.04 has 2.39. Defined here, the linker never
//! asks glibc for them. Denial packages Settings for those systems and
//! checks the program against its glibc baseline; a function that check
//! names belongs here.

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
    asinf(x) -> f32;
    atan2f(y, x) -> f32;
    coshf(x) -> f32;
    sinhf(x) -> f32;
    cosh(x) -> f64;
    sinh(x) -> f64;
}
