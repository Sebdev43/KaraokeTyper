/// Dev-only logging macro - compiles to nothing in release builds
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! dev_log {
    ($($arg:tt)*) => { println!($($arg)*) }
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! dev_log {
    ($($arg:tt)*) => { () }
}
