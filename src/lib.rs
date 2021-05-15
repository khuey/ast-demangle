//! # ast-demangle
//!
//! Parse mangled names and produce structed results.
//!
//! Example:
//!
//! ```rust
//! use ast_demangle::rust_v0::display::Style;
//! use ast_demangle::rust_v0::{Identifier, Path, Symbol};
//!
//! let mangled_name = "_RNvNtCs6GSVXm7oiwY_5regex4utf811decode_utf8.llvm.1119170478327948870";
//! let (symbol, suffix) = Symbol::parse_from_str(mangled_name).unwrap();
//!
//! // The suffix is returned.
//! assert_eq!(suffix, ".llvm.1119170478327948870");
//!
//! // The default style for displaying is the long format.
//! assert_eq!(format!("{}", symbol), "regex[4df147058689a776]::utf8::decode_utf8");
//!
//! // To omit the crate hash, use the alternative display format.
//! assert_eq!(format!("{:#}", symbol), "regex::utf8::decode_utf8");
//!
//! // Use `Symbol::display` and `Style` to specify the display style explicitly.
//!
//! assert_eq!(format!("{}", symbol.display(Style::Short)), "decode_utf8");
//! assert_eq!(format!("{}", symbol.display(Style::Normal)), "regex::utf8::decode_utf8");
//!
//! assert_eq!(
//!     format!("{}", symbol.display(Style::Long)),
//!     "regex[4df147058689a776]::utf8::decode_utf8"
//! );
//!
//! // You can access the structure of the demangled symbol.
//!
//! assert_eq!(
//!     symbol,
//!     Symbol {
//!         version: None,
//!         path: Path::Nested {
//!             namespace: b'v',
//!             path: Path::Nested {
//!                 namespace: b't',
//!                 path: Path::CrateRoot(Identifier {
//!                     disambiguator: 0x4df1_4705_8689_a776,
//!                     name: "regex".into()
//!                 })
//!                 .into(),
//!                 name: Identifier {
//!                     disambiguator: 0,
//!                     name: "utf8".into()
//!                 }
//!             }
//!             .into(),
//!             name: Identifier {
//!                 disambiguator: 0,
//!                 name: "decode_utf8".into()
//!             }
//!         }
//!         .into(),
//!         instantiating_crate: None
//!     }
//! );
//! ```

#![warn(
    clippy::clone_on_ref_ptr,
    clippy::needless_borrow,
    clippy::pedantic,
    clippy::rc_buffer,
    clippy::suboptimal_flops,
    clippy::useless_let_if_seq
)]
#![allow(clippy::non_ascii_literal)]

pub mod rust_v0;
