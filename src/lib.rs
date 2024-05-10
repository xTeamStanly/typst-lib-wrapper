#![allow(clippy::needless_return)]

mod builder;
mod compiler;
mod errors;
mod files;
mod fonts;
mod package; // DONE
mod parameters;

// Re-exports
pub use native_tls::Certificate;

pub use builder::CompilerBuilder;
pub use compiler::Compiler;
pub use errors::WrapperError as Error;
pub use fonts::FontCache;
pub use parameters::{Input, Format, Output};
