use crate::api::telemetry;
use crate::cli::TrackArgs;
use crate::commands::{bad, good, table, warn_cell};
use anyhow::Result;
use comfy_table::Cell;

pub fn run(args: TrackArgs, json: bool) -> Result<()> {
    let mut fleet = telemetry::fleet()?;

    if let Some(filter) = &args.train {
        let f = filter.to_lowercase();
        fleet.retain(|t| {
            t.train_number.to_lowercase().contains(&f) || t.service.to_lowercase().contains(&f)
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&fleet)?);
        return Ok(());
    }

    if fleet.is_empty() {
        println!("Sin trenes activos que coincidan.");
        return Ok(());
    }
    let mut t = table(&["Tren", "Servicio", "Retraso", "Anterior", "Siguiente"]);
    for p in &fleet {
        let delay = format!("{} min", p.delay_min);
        let delay_cell = match p.delay_min {
            d if d <= 0 => good(delay),
            d if d <= 5 => warn_cell(delay),
            _ => bad(delay),
        };
        t.add_row(vec![
            Cell::new(&p.train_number),
            Cell::new(&p.service),
            delay_cell,
            Cell::new(p.last_station.clone().unwrap_or_else(|| "—".into())),
            Cell::new(p.next_station.clone().unwrap_or_else(|| "—".into())),
        ]);
    }
    println!("{t}");
    Ok(())
}
