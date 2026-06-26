pub mod buy;
pub mod profile;
pub mod search;
pub mod stations;
pub mod track;

use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, Table};

/// Construye una tabla con el preset estándar del CLI, con la cabecera
/// resaltada en color para que destaque sobre las filas de datos.
pub fn table(header: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL);
    t.set_header(header.iter().map(|h| {
        Cell::new(*h).fg(Color::Cyan).add_attribute(Attribute::Bold)
    }));
    t
}

/// Celda en verde (éxito / disponible / sin retraso).
pub fn good(s: impl Into<String>) -> Cell {
    Cell::new(s.into()).fg(Color::Green)
}

/// Celda en rojo (error / sin plaza / cancelado).
pub fn bad(s: impl Into<String>) -> Cell {
    Cell::new(s.into()).fg(Color::Red)
}

/// Celda en amarillo (aviso / situación intermedia).
pub fn warn_cell(s: impl Into<String>) -> Cell {
    Cell::new(s.into()).fg(Color::Yellow)
}

/// Celda destacada en negrita (valor importante, p. ej. un precio).
pub fn bold(s: impl Into<String>) -> Cell {
    Cell::new(s.into()).add_attribute(Attribute::Bold)
}
