#![allow(clippy::needless_return)]

mod builder;
mod compiler; // DONE
mod errors; // DONE
mod files; // DONE
mod fonts;
mod package; // DONE
mod parameters; // DONE

/// Necessary re-exports for completeness. \
/// Typst errors, tls certificate, ...
pub mod reexports {
    pub use native_tls::Certificate;

    pub use ecow::{EcoString, EcoVec};

    pub use typst::layout::{Abs, Angle, Em, Length, Ratio, Rel};
    pub use typst::util::{PicoStr, Scalar, Static};

    pub use typst::diag::{PackageError, FileError, SourceDiagnostic};
    pub use typst::eval::Tracer;
    pub use typst::foundations::{
        Arg, Args, Array, Bytes, Content, Datetime, Dict, Duration, Dynamic, Func, IndexMap,
        IntoValue, Label, Module, NativeTypeData, Plugin, Str, Style, Styles, Type, Value, Version,
        Element, NativeElement, NativeElementData
    };

    pub use typst::visualize::{
        Cmyk, Color, Gradient, Hsl, Hsv, LinearRgb, Luma, Oklab, Oklch, Pattern, Rgb
    };
    pub use typst_syntax::Span;
}

pub use builder::CompilerBuilder;
pub use compiler::Compiler;
pub use errors::WrapperError;
pub use fonts::FontCache;
pub use parameters::{CompilerOutput, Input};
