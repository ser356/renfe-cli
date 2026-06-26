pub mod buy;
pub mod profile;
pub mod search;
pub mod stations;
pub mod track;

use comfy_table::{presets::UTF8_FULL, Table};

/// Construye una tabla con el preset estándar del CLI.
pub fn table(header: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL);
    t.set_header(header.iter().map(|h| h.to_string()).collect::<Vec<_>>());
    t
}
