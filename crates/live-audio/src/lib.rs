//! Control-thread audio configuration; this crate does not open hardware.
#[cfg(windows)]
pub mod discovery;
pub mod routing;
