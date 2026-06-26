use crate::api::stations;
use crate::cli::StationsArgs;
use crate::commands::table;
use anyhow::Result;
use colored::Colorize;
use comfy_table::{Attribute, Cell};

pub fn run(args: StationsArgs, json: bool) -> Result<()> {
    let catalog = stations::load(args.refresh)?;
    let filtered: Vec<_> = match &args.query {
        Some(q) => {
            let q = q.to_lowercase();
            catalog
                .into_iter()
                .filter(|s| s.name.to_lowercase().contains(&q) || s.code == *q)
                .collect()
        }
        None => catalog,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    if filtered.is_empty() {
        println!("{}", "Sin coincidencias.".yellow());
        return Ok(());
    }
    let mut t = table(&["Código", "Estación"]);
    for s in &filtered {
        t.add_row(vec![
            Cell::new(&s.code).add_attribute(Attribute::Dim),
            Cell::new(&s.name),
        ]);
    }
    println!("{t}");
    Ok(())
}
