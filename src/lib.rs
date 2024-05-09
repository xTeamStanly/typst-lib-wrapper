#![allow(clippy::needless_return)]


mod asynchronous;


// mod compiler_options;
// mod system_font_cache;

pub mod errors;

pub mod shared;

pub mod blocking;

// #[cfg(feature = "async")]
// mod async_impl;

// #[cfg(feature = "sync")]
// mod sync_impl;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}
