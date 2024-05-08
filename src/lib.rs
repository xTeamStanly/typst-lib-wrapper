#![allow(clippy::needless_return)]


mod asynchronous;


// mod compiler_options;
// mod system_font_cache;

mod errors;
pub use errors::{WrapperError, WrapperResult};

pub mod shared;

pub mod blocking;

// #[cfg(feature = "async")]
// mod async_impl;

// #[cfg(feature = "sync")]
// mod sync_impl;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}
