use crate::api::sale::{self, BuyParams, SearchParams};
use crate::api::session::Session;
use crate::api::stations;
use crate::cli::BuyArgs;
use crate::commands::{bold, good, bad, table};
use crate::config;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use comfy_table::Cell;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

/// Fuente del helper que abre un navegador real con la sesión inyectada.
/// Embebida en el binario para que `renfe buy --open` funcione sin depender
/// de tener el repo a mano; ver `tools/open_checkout.py` para el original.
const OPEN_CHECKOUT_SCRIPT: &str = include_str!("../../tools/open_checkout.py");

pub fn run(args: BuyArgs, json: bool, profile_name: Option<&str>) -> Result<()> {
    let profile = config::resolve(profile_name)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no hay perfil activo: necesito uno con datos del viajero. \
             Crea uno con `renfe profile set yo --nombre ... --apellido1 ... \
             --documento ... --tipo-documento dni --email ... --telefono ...`"
        )
    })?;

    let catalog = stations::load(false)?;
    let origin = stations::resolve(&args.origin, &catalog)?;
    let destination = stations::resolve(&args.destination, &catalog)?;
    let date = args
        .date
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    // ── Modo interactivo: buscar y elegir tren ──────────────────────────────
    let (train_id, fare_code_owned) = if let Some(id) = args.train {
        (id, args.fare.clone())
    } else {
        if json {
            bail!("--train es obligatorio en modo --json");
        }
        eprintln!(
            "{} {} {} {} {}...",
            "Buscando".cyan(),
            origin.name.bold(),
            "→".cyan(),
            destination.name.bold(),
            format!("el {date}").cyan()
        );
        let session_search = Session::new(config::resolve(profile_name)?)?;
        let sp = SearchParams {
            origin: &origin,
            destination: &destination,
            date: &date,
            return_date: None,
            adults: 1,
        };
        let mut trains = sale::search(&session_search, &sp)?;
        trains.retain(|t| t.available);
        if trains.is_empty() {
            bail!("no hay trenes disponibles para esa ruta y fecha");
        }

        // Mostrar tabla numerada
        let mut t = table(&["#", "Tipo", "Tren", "Salida", "Llegada", "Duración", "Precio"]);
        for (i, tr) in trains.iter().enumerate() {
            t.add_row(vec![
                Cell::new(i + 1),
                Cell::new(&tr.train_type),
                Cell::new(&tr.train_number),
                Cell::new(&tr.departure),
                Cell::new(&tr.arrival),
                Cell::new(&tr.duration),
                tr.price
                    .map(|p| bold(format!("{p:.2}€")))
                    .unwrap_or_else(|| Cell::new("—")),
            ]);
        }
        eprintln!("{t}");

        let idx = ask_index("Elige tren", trains.len())?;
        let chosen_train = &trains[idx];

        // Elegir tarifa
        let chosen_fare_name = if args.fare.is_none() && chosen_train.fares.len() > 1 {
            let mut ft = table(&["#", "Tarifa", "Clase", "Precio", "Disponible"]);
            for (i, f) in chosen_train.fares.iter().enumerate() {
                ft.add_row(vec![
                    Cell::new(i + 1),
                    Cell::new(&f.name),
                    Cell::new(&f.class),
                    bold(format!("{:.2}€", f.price)),
                    if f.available { good("sí") } else { bad("no") },
                ]);
            }
            eprintln!("{ft}");
            let fi = ask_index("Elige tarifa", chosen_train.fares.len())?;
            Some(chosen_train.fares[fi].name.clone())
        } else {
            args.fare.clone()
        };

        let tid = chosen_train
            .train_number
            .parse::<i64>()
            .with_context(|| format!("número de tren «{}» no es numérico", chosen_train.train_number))?;
        (tid, chosen_fare_name)
    };
    let fare_code_ref = fare_code_owned.as_deref();

    if !json {
        eprintln!(
            "{} {} {} {} el {} (tren id={}{}). Titular: {} {} {}.",
            "Compra:".bold().cyan(),
            origin.name.bold(),
            "→".cyan(),
            destination.name.bold(),
            date,
            train_id,
            fare_code_ref
                .map(|f| format!(", tarifa {f}"))
                .unwrap_or_default(),
            profile.nombre.as_deref().unwrap_or("?"),
            profile.apellido1.as_deref().unwrap_or("?"),
            profile.apellido2.as_deref().unwrap_or(""),
        );
        if !args.yes {
            confirm(&format!(
                "{} [s/N] ",
                "Continuar y armar el carrito (no se cobra)?".yellow()
            ))?;
        }
    }

    let session = Session::new(Some(profile.clone()))?;
    let params = BuyParams {
        origin: &origin,
        destination: &destination,
        date: &date,
        train_id,
        fare_code: fare_code_ref,
        viajero: &profile,
    };

    let outcome = sale::buy(&session, &params).context("armando carrito de compra")?;

    let cookies_path = args
        .cookies_out
        .clone()
        .unwrap_or_else(|| format!("./renfe-buy-{}.cookies.txt", outcome.id_compra));
    write_cookies_netscape(&cookies_path, &outcome.cookies_header)
        .with_context(|| format!("escribiendo {cookies_path}"))?;

    if args.open {
        open_browser_with_session(
            &cookies_path,
            &outcome.checkout_url,
            args.bizum,
            profile.email.as_deref(),
            profile.telefono.as_deref(),
        )?;
    }

    if json {
        // No volcamos el header Cookie en JSON: contiene la sesión completa.
        // Solo damos URL e id; el cookies.txt está en disco.
        let payload = serde_json::json!({
            "id_compra": outcome.id_compra,
            "checkout_url": outcome.checkout_url,
            "cookies_file": cookies_path,
            "train": outcome.chosen_train,
            "fare": outcome.chosen_fare,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let mut t = table(&["Campo", "Valor"]);
    t.add_row(vec!["idCompra".to_string(), outcome.id_compra.clone()]);
    t.add_row(vec![
        "Tren".into(),
        format!(
            "{} #{}  {} → {} ({})",
            outcome.chosen_train.train_type,
            outcome.chosen_train.train_number,
            outcome.chosen_train.departure,
            outcome.chosen_train.arrival,
            outcome.chosen_train.duration,
        ),
    ]);
    t.add_row(vec![
        "Tarifa".into(),
        format!(
            "{} ({}) — {:.2}€",
            outcome.chosen_fare.name,
            outcome.chosen_fare.class,
            outcome.chosen_fare.price
        ),
    ]);
    t.add_row(vec!["URL de pago".into(), outcome.checkout_url.clone()]);
    t.add_row(vec!["Cookies".into(), cookies_path.clone()]);
    println!("{t}");
    println!();
    println!("{}", "Carrito armado. Para pagar:".green().bold());
    println!("  1. Abre la URL anterior en el navegador.");
    println!(
        "  2. Si la sesión no se ata, importa cookies con: \
         curl -b {cookies_path} -c {cookies_path} '{}'",
        outcome.checkout_url
    );
    println!("  El pago (Redsys + 3DS) NO se automatiza, se hace en el navegador.");
    eprintln!();
    eprintln!(
        "{} {cookies_path} contiene cookies de sesión sensibles. Bórralo al terminar.",
        "Aviso:".yellow().bold()
    );
    Ok(())
}

/// Convierte un header `Cookie: a=1; b=2; c=3` al formato Netscape cookies.txt
/// que entienden curl, wget y Firefox.
fn write_cookies_netscape(path: &str, cookie_header: &str) -> Result<()> {
    if cookie_header.trim().is_empty() {
        bail!("la sesión no produjo cookies: ¿falló algún paso intermedio?");
    }
    let mut out = String::new();
    out.push_str("# Netscape HTTP Cookie File\n");
    out.push_str("# Generado por renfe-cli. Sensible: no compartir.\n\n");
    // Renfe sirve cookies en .renfe.com y venta.renfe.com. Sin más metadata
    // (el header `Cookie:` no la trae), las exportamos contra venta.renfe.com,
    // que es el host del flujo de pago.
    const DOMAIN: &str = "venta.renfe.com";
    // Expira en una hora (la sesión de Renfe es corta de todas formas).
    let expires = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 3600) as i64;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (name, value) = match pair.split_once('=') {
            Some(nv) => nv,
            None => continue,
        };
        // domain | flag | path | secure | expires | name | value
        out.push_str(&format!(
            "{}\tFALSE\t/\tTRUE\t{}\t{}\t{}\n",
            DOMAIN,
            expires,
            name.trim(),
            value.trim()
        ));
    }
    fs::write(path, out)?;
    Ok(())
}

/// Lanza el helper de Python embebido para abrir un navegador real (visible,
/// no headless) ya logueado en `checkout_url`. El navegador queda abierto
/// para que la persona pague a mano; este proceso solo espera a que el
/// script termine de inyectar las cookies y navegar.
///
/// Con `auto_pay=true`, el script rellena automáticamente el formulario de
/// Redsys (tarjeta y CVV) y espera la aprobación 3DS en el móvil. El CVV
/// lo pide el propio script en la terminal vía `getpass`; nunca pasa por
/// argumentos ni por disco.
fn open_browser_with_session(
    cookies_path: &str,
    checkout_url: &str,
    bizum: bool,
    buyer_email: Option<&str>,
    buyer_phone: Option<&str>,
) -> Result<()> {
    let script_path = std::env::temp_dir().join("renfe-open-checkout.py");
    fs::write(&script_path, OPEN_CHECKOUT_SCRIPT)
        .with_context(|| format!("escribiendo script temporal en {}", script_path.display()))?;

    eprintln!("{}", "Abriendo navegador con la sesión del carrito...".cyan());

    let mut cmd = Command::new("python3");
    cmd.arg(&script_path)
        .arg("--cookies")
        .arg(cookies_path)
        .arg("--url")
        .arg(checkout_url);

    if bizum {
        cmd.arg("--bizum");
    }
    if let Some(email) = buyer_email {
        cmd.env("RENFE_BUYER_EMAIL", email);
    }
    if let Some(phone) = buyer_phone {
        cmd.env("RENFE_BUYER_PHONE", phone);
    }

    let status = cmd
        .status()
        .context("no se pudo ejecutar `python3`. ¿Está instalado y en el PATH?")?;
    if !status.success() {
        bail!(
            "el navegador no se abrió (python3 salió con código {}). \
             El script instala `selenium` solo si falta; revisa el mensaje \
             anterior. Como alternativa, importa manualmente {cookies_path} \
             en el navegador.",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn confirm(prompt: &str) -> Result<()> {
    eprint!("{prompt}");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_lowercase();
    if answer == "s" || answer == "si" || answer == "sí" || answer == "y" || answer == "yes" {
        Ok(())
    } else {
        bail!("cancelado por el usuario")
    }
}

/// Pide al usuario un número del 1 al `max` y devuelve el índice 0-based.
fn ask_index(label: &str, max: usize) -> Result<usize> {
    loop {
        eprint!("{} [1-{}]: ", label.yellow().bold(), max);
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let s = line.trim();
        match s.parse::<usize>() {
            Ok(n) if n >= 1 && n <= max => return Ok(n - 1),
            _ => eprintln!("  {}", format!("Introduce un número entre 1 y {max}.").red()),
        }
    }
}
