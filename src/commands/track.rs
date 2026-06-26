use crate::api::telemetry;
use crate::cli::TrackArgs;
use crate::commands::table;
use anyhow::Result;

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
        t.add_row(vec![
            p.train_number.clone(),
            p.service.clone(),
            format!("{} min", p.delay_min),
            p.last_station.clone().unwrap_or_else(|| "—".into()),
            p.next_station.clone().unwrap_or_else(|| "—".into()),
        ]);
    }
    println!("{t}");
    Ok(())
}
