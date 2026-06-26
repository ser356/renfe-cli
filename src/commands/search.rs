use crate::api::sale::{self, SearchParams};
use crate::api::session::Session;
use crate::api::stations;
use crate::cli::SearchArgs;
use crate::commands::{bad, bold, good, table};
use crate::config;
use anyhow::Result;
use colored::Colorize;
use comfy_table::Cell;

pub fn run(args: SearchArgs, json: bool, profile: Option<&str>) -> Result<()> {
    let catalog = stations::load(false)?;
    let origin = stations::resolve(&args.origin, &catalog)?;
    let destination = stations::resolve(&args.destination, &catalog)?;
    let date = args
        .date
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    if !json {
        eprintln!(
            "{} {} {} {} {}...",
            "Buscando".cyan(),
            origin.name.bold(),
            "→".cyan(),
            destination.name.bold(),
            format!("el {date}").cyan()
        );
    }

    let session = Session::new(config::resolve(profile)?)?;
    let params = SearchParams {
        origin: &origin,
        destination: &destination,
        date: &date,
        return_date: args.return_date.as_deref(),
        adults: args.adults,
    };

    let mut trains = sale::search(&session, &params)?;

    if args.available_only {
        trains.retain(|t| t.available);
    }
    match args.sort.as_str() {
        "precio" => trains.sort_by(|a, b| {
            a.price
                .unwrap_or(f64::MAX)
                .partial_cmp(&b.price.unwrap_or(f64::MAX))
                .unwrap()
        }),
        "duracion" => trains.sort_by(|a, b| a.duration.cmp(&b.duration)),
        _ => trains.sort_by(|a, b| a.departure.cmp(&b.departure)),
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&trains)?);
        return Ok(());
    }

    let mut t = table(&["Tipo", "Tren", "Salida", "Llegada", "Duración", "Precio", "Plaza"]);
    for tr in &trains {
        t.add_row(vec![
            Cell::new(&tr.train_type),
            Cell::new(&tr.train_number),
            Cell::new(&tr.departure),
            Cell::new(&tr.arrival),
            Cell::new(&tr.duration),
            tr.price
                .map(|p| bold(format!("{p:.2}€")))
                .unwrap_or_else(|| Cell::new("—")),
            if tr.available { good("sí") } else { bad("no") },
        ]);
    }
    println!("{t}");
    Ok(())
}
