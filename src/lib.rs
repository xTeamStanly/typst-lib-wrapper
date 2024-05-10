#![allow(clippy::needless_return)]

mod builder;
mod compiler;
mod errors;
mod files;
mod fonts;
mod package; // DONE
mod parameters;

/// Necessary re-exports for completeness. \
/// Typst errors, tls certificate, ...
pub mod reexports {
    pub use native_tls::Certificate;

    pub use ecow::{EcoString, EcoVec};

    pub use typst::diag::{PackageError, SourceDiagnostic};
    pub use typst::eval::Tracer;
    pub use typst::foundations::{IntoValue, Styles, Value};
    pub use typst::visualize::{Cmyk, Color, Hsl, Hsv, LinearRgb, Luma, Oklab, Oklch, Rgb};
    pub use typst_syntax::Span;
}

pub use builder::CompilerBuilder;
pub use compiler::Compiler;
pub use errors::WrapperError;
pub use fonts::FontCache;
pub use parameters::{CompilerOutput, Format, Input};
