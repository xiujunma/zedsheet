//! Clipboard interchange for cross-app copy/paste (Excel, Google Sheets, etc.).
//!
//! Two flavors are written to / read from the system clipboard:
//! - `text/plain` as TSV — preserves the grid structure (rows/columns) and
//!   carries the cell's raw text, so a formula like `=SUM(A1:A3)` lands in
//!   Excel as a live formula.
//! - `text/html` as a `<table>` — additionally preserves merged cells
//!   (colspan/rowspan) and per-cell styling, and embeds a nonce so we can
//!   recognize our own clipboard payload and paste it losslessly in-app.
//!
//! Everything here is pure (no DOM) and host-tested. The browser glue that
//! reads/writes the clipboard and walks pasted HTML lives in
//! `zedsheet::system_clipboard`.

pub mod model;
pub mod parse;
pub mod serialize;

pub use model::ParsedGrid;
pub use parse::{grid_from_rows, nonce_in_html, parse_tsv, RawCell};
pub use serialize::{to_html, to_tsv};
