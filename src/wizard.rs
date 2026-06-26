//! Asistente de configuración inicial: se lanza solo la primera vez que se
//! ejecuta el CLI (no existe `~/.renfe/profiles.json`) y crea el primer
//! perfil con lo mínimo para usar `renfe search`/`renfe buy`. Pensado para
//! rellenarse rápido: todo tiene un valor por defecto razonable salvo el
//! nombre y los apellidos, y se puede repetir/corregir luego con
//! `renfe profile set`.

use crate::cli::Command;
use crate::config::{self, Profile};
use anyhow::Result;
use colored::Colorize;
use std::io::{self, IsTerminal, Write};

/// Si es la primera vez que se ejecuta el CLI (no hay `profiles.json` todavía)
/// y la entrada/salida es interactiva, ofrece crear el primer perfil. No
/// interrumpe `--json` (pensado para scripting) ni `renfe profile ...`
/// (el usuario ya está gestionando perfiles a mano).
pub fn maybe_run(json: bool, command: &Command) -> Result<()> {
    if json || matches!(command, Command::Profile(_)) {
        return Ok(());
    }
    if config::profiles_file_exists()? {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        // No hay nadie delante del teclado (script, pipe, CI): no bloqueamos.
        return Ok(());
    }
    run()
}

fn run() -> Result<()> {
    eprintln!();
    eprintln!("{}", "¡Bienvenido a renfe-cli!".bold().green());
    eprintln!(
        "{}",
        "No hay ningún perfil configurado todavía. Vamos a crear el primero.".dimmed()
    );
    eprintln!(
        "{}",
        "Pulsa Enter para aceptar el valor por defecto entre [corchetes], o deja en blanco para omitir.".dimmed()
    );
    eprintln!();

    let name = ask_default("Nombre del perfil", "yo")?;
    let nombre = ask_optional("Nombre del titular (para `renfe buy`)")?;
    let apellido1 = ask_optional("Primer apellido")?;
    let apellido2 = ask_optional("Segundo apellido")?;
    let tipo_documento = ask_choice(
        "Tipo de documento",
        &["dni", "nie", "pasaporte"],
        "dni",
    )?;
    let documento = ask_optional("Número de documento")?;
    let email = ask_optional("Email de la cuenta Renfe")?;
    let prefijo = ask_default("Prefijo telefónico", "+34")?;
    let telefono = ask_optional("Teléfono móvil (sin prefijo)")?;
    let token = ask_optional("Token/cookie de sesión (ver CAPTURA.md; puedes añadirlo luego)")?;

    let profile = Profile {
        name: name.clone(),
        email,
        token,
        nombre,
        apellido1,
        apellido2,
        tipo_documento: Some(tipo_documento_to_code(&tipo_documento).to_string()),
        documento,
        prefijo: Some(prefijo),
        telefono,
    };

    let mut store = config::load()?;
    store.profiles.retain(|p| p.name != name);
    store.profiles.push(profile);
    store.active = Some(name.clone());
    config::save(&store)?;

    eprintln!();
    eprintln!(
        "{} Perfil «{}» creado y marcado como activo.",
        "✓".green().bold(),
        name.bold()
    );
    eprintln!(
        "{}",
        "Puedes completarlo o cambiarlo en cualquier momento con `renfe profile set ...`."
            .dimmed()
    );
    eprintln!();
    Ok(())
}

fn tipo_documento_to_code(s: &str) -> &'static str {
    match s {
        "nie" => "0023",
        "pasaporte" => "0014",
        _ => "0021",
    }
}

fn ask_default(label: &str, default: &str) -> Result<String> {
    eprint!("{} {} ", label.cyan(), format!("[{default}]:").dimmed());
    io::stderr().flush().ok();
    let line = read_line()?;
    Ok(if line.trim().is_empty() {
        default.to_string()
    } else {
        line.trim().to_string()
    })
}

fn ask_optional(label: &str) -> Result<Option<String>> {
    eprint!("{} {} ", label.cyan(), "(opcional):".dimmed());
    io::stderr().flush().ok();
    let line = read_line()?;
    let trimmed = line.trim();
    Ok(if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    })
}

fn ask_choice(label: &str, options: &[&str], default: &str) -> Result<String> {
    loop {
        eprint!(
            "{} {} {} ",
            label.cyan(),
            format!("({})", options.join("/")).dimmed(),
            format!("[{default}]:").dimmed()
        );
        io::stderr().flush().ok();
        let line = read_line()?;
        let trimmed = line.trim().to_lowercase();
        if trimmed.is_empty() {
            return Ok(default.to_string());
        }
        if options.contains(&trimmed.as_str()) {
            return Ok(trimmed);
        }
        eprintln!("  {} valor no válido, elige una de: {}", "✗".red(), options.join(", "));
    }
}

fn read_line() -> Result<String> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line)
}
