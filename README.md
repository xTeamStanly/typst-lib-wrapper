**Note**: This library is intended for personal use. It will not be added to `crates.io`. \
If you wish to use this:
```toml
typst-lib-wrapper = { git = "https://github.com/xTeamStanly/typst-lib-wrapper.git" }
```

# Overview
Rust library that synchronously wraps Typst compiler, enabling you to compile documents and
capture output in code.

# Features

-   ✅ **Capture compilation result**:
    Typst projects are compiled in memory, compilation result is immediately available as a
    collection of bytes. You don't have to invoke the [official CLI][typst-cli]. Compiler input
    can be either **Content** (string) or **File** (entry filename and project root).

-   🔃 **Custom data loading**:
    Provides a way to override Typst standard library and add custom symbols to the global context.
    This will overload **ANY** symbol, use it with caution. It is recommended that
    **all custom data starts with underscore "_".** Another way to load custom data is
    to use `sys.inputs` dictionary, but it's limited as it only allows strings as values.

-   🔠 **Lazily loaded fonts**:
    Fonts are loaded lazily on-demand. Font cache keeps all fonts metadata in memory, but the
    actual font data is lazy loaded.

-   🗃️ **Font caching**:
    Global font cache keeps track of all fonts. The cache is updated if new fonts are loaded after
    compilation.

-   📜 **Fully documented with examples**:
    This library is fully documented and important functions are documented with examples.

# Examples

### PDF compilation
```rust
let entry = "main.typ";
let root = "./project";

// Build the compiler and compile to PDF.
let compiler = CompilerBuilder::with_file_input(entry, root)
    .build()
    .expect("Couldn't build the compiler");
let compiled = compiler.compile_pdf();

if let Some(pdf) = compiled.output {
    std::fs::write("./main.pdf", pdf)
        .expect("Couldn't write PDF"); // Writes PDF file.
} else {
    dbg!(compiled.errors); // Compilation failed, show errors.
}
```

### PNG compilation
```rust
let entry = "main.typ";
let root = "./project";

// Build the compiler and compile to PNG.
let compiler = CompilerBuilder::with_file_input(entry, root)
    .build()
    .expect("Couldn't build the compiler");
let compiled = compiler.compile_png();

if let Some(pages) = compiled.output {
    // Writes images one by one.
    pages.iter().enumerate().for_each(|(index, page)| {
        let filename = format!("./output/{index}.png");
        std::fs::write(filename, page)
            .expect("Couldn't write PNG");
    });
} else {
    dbg!(compiled.errors); // Compilation failed, show errors.
}
```

### Custom fonts
```rust
// Add fonts to cache
let font_paths = vec![
    "./assets/fonts/times_new_roman.ttf",
    "~/path/to/custom/fonts/comic_sans.ttf"
];
FontCache::insert_many(font_paths)
    .expect("Cache error");

let content = r##"
    #set page(paper: "a4");

    #set text(font: "Times New Roman");
    This is Times New Roman.

    #set text(font: "Comic Sans");
    This is Comic Sans.
"##;

let compiler = CompilerBuilder::with_content_input(content)
    .with_font_paths(font_paths)
    .build()
    .expect("Couldn't build the compiler");
let compiled = compiler.compile_pdf();

if let Some(pdf) = compiled.output {
    std::fs::write("./main.pdf", pdf)
        .expect("Couldn't write PDF"); // Writes PDF file.
} else {
    dbg!(compiled.errors); // Compilation failed, show errors.
}
```

### Custom data
This creates a document with text **_"Rust 1.0 was released on 15.05.2015."_**.
The text "Rust" is orange.
Currently typst floats aren't displaying a decimal
point when passed a floating point that can be converted to integer without loss.
That's why there's `".0"` after `"#_VERSION"`, it is not a tuple index.

```rust
use typst_lib_wrapper::reexports::{IntoValue, Datetime, Color};

let content = r##"
    #set page(paper: "a4");

    #text(fill: _COLOR)[#_LANGUAGE]
    #_VERSION.0 was released on
    #_DATE.display("[day].[month].[year]").
"##;

let language = "Rust".into_value();
let date = Datetime::from_ymd(2015, 5, 15).expect("Invalid date").into_value();
let color = Color::from_u8(247, 75, 0, 255).into_value();
let version = 1.0_f64.into_value();

let custom_data = vec![
    ("_LANGUAGE", language),
    ("_DATE", date),
    ("_COLOR", color),
    ("_VERSION", version)
];

let compiler = CompilerBuilder::with_content_input(content)
    .with_custom_data(custom_data)
    .build()
    .expect("Couldn't build the compiler");
let compiled = compiler.compile_pdf();

if let Some(pdf) = compiled.output {
    std::fs::write("./main.pdf", pdf)
        .expect("Couldn't write PDF"); // Writes PDF file.
} else {
    dbg!(compiled.errors); // Compilation failed, show errors.
}
```

There are more specific examples alongside every functions.

# Notes / Warnings

-   ⌚ **Synchronous**:
    Every mutex in this library is sync `parking_lot::Mutex`. Meaning, font caching and parallel
    PNG/SVG compilation are **blocking**. Use **`blocking_task`** provided by your async runtime if
    you with to compile documents in an async environment while interracting with this library.

-   🔀 **Font cache cloning**:
    Before building the compiler, builder locks the cache and **clones** it for itself. To reduce
    cache size it is recommended to avoid loading all system fonts and to load only needed fonts.
    In practise this won't be that big of a deal, because all fonts are lazily loaded on-demand.
    But once loaded they stay in cache, so **manually emptying** is an option.

-   📥 **Automatic package downloads**: This library will automatically download packages from the
    Typst package registry. This is also done by the [official CLI][typst-cli].




[typst-cli]: https://github.com/typst/typst/tree/main/crates/typst-cli