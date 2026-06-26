use crate::api::sale::{self, BuyParams};
use crate::api::session::Session;
use crate::api::stations;
use crate::cli::BuyArgs;
use crate::commands::table;
use crate::config;
use anyhow::{bail, Context, Result};
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

    if !json {
        eprintln!(
            "Compra: {} → {} el {} (tren id={}{}). Titular: {} {} {}.",
            origin.name,
            destination.name,
            args.date,
            args.train,
            args.fare
                .as_deref()
                .map(|f| format!(", tarifa {f}"))
                .unwrap_or_default(),
            profile.nombre.as_deref().unwrap_or("?"),
            profile.apellido1.as_deref().unwrap_or("?"),
            profile.apellido2.as_deref().unwrap_or(""),
        );
        if !args.yes {
            confirm("Continuar y armar el carrito (no se cobra)? [s/N] ")?;
        }
    }

    let session = Session::new(Some(profile.clone()))?;
    let params = BuyParams {
        origin: &origin,
        destination: &destination,
        date: &args.date,
        train_id: args.train,
        fare_code: args.fare.as_deref(),
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
        open_browser_with_session(&cookies_path, &outcome.checkout_url)?;
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
    println!("Carrito armado. Para pagar:");
    println!("  1. Abre la URL anterior en el navegador.");
    println!(
        "  2. Si la sesión no se ata, importa cookies con: \
         curl -b {cookies_path} -c {cookies_path} '{}'",
        outcome.checkout_url
    );
    println!("  El pago (Redsys + 3DS) NO se automatiza, se hace en el navegador.");
    eprintln!();
    eprintln!("Aviso: {cookies_path} contiene cookies de sesión sensibles. Bórralo al terminar.");
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
fn open_browser_with_session(cookies_path: &str, checkout_url: &str) -> Result<()> {
    let script_path = std::env::temp_dir().join("renfe-open-checkout.py");
    fs::write(&script_path, OPEN_CHECKOUT_SCRIPT)
        .with_context(|| format!("escribiendo script temporal en {}", script_path.display()))?;

    eprintln!("Abriendo navegador con la sesión del carrito...");
    let status = Command::new("python3")
        .arg(&script_path)
        .arg("--cookies")
        .arg(cookies_path)
        .arg("--url")
        .arg(checkout_url)
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
