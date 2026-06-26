use crate::cli::{ProfileAction, ProfileArgs};
use crate::commands::table;
use crate::config::{self, Profile};
use anyhow::{bail, Result};

pub fn run(args: ProfileArgs, json: bool) -> Result<()> {
    match args.action {
        ProfileAction::List => list(json),
        ProfileAction::Set {
            name,
            token,
            email,
            nombre,
            apellido1,
            apellido2,
            tipo_documento,
            documento,
            prefijo,
            telefono,
        } => set(
            SetArgs {
                name,
                token,
                email,
                nombre,
                apellido1,
                apellido2,
                tipo_documento,
                documento,
                prefijo,
                telefono,
            },
            json,
        ),
        ProfileAction::Use { name } => use_profile(name, json),
    }
}

struct SetArgs {
    name: String,
    token: Option<String>,
    email: Option<String>,
    nombre: Option<String>,
    apellido1: Option<String>,
    apellido2: Option<String>,
    tipo_documento: Option<String>,
    documento: Option<String>,
    prefijo: Option<String>,
    telefono: Option<String>,
}

fn tipo_documento_to_code(s: &str) -> &'static str {
    match s {
        "dni" => "0021",
        "nie" => "0023",
        "pasaporte" => "0014",
        _ => "0021",
    }
}

fn list(json: bool) -> Result<()> {
    let store = config::load()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&store)?);
        return Ok(());
    }
    if store.profiles.is_empty() {
        println!("No hay perfiles. Crea uno con: renfe profile set <nombre> --token ...");
        return Ok(());
    }
    let mut t = table(&["Activo", "Nombre", "Email", "Token"]);
    for p in &store.profiles {
        t.add_row(vec![
            if store.active.as_deref() == Some(&p.name) { "►".into() } else { "".into() },
            p.name.clone(),
            p.email.clone().unwrap_or_default(),
            if p.token.is_some() { "sí".into() } else { "no".into() },
        ]);
    }
    println!("{t}");
    Ok(())
}

fn set(args: SetArgs, json: bool) -> Result<()> {
    let SetArgs {
        name,
        token,
        email,
        nombre,
        apellido1,
        apellido2,
        tipo_documento,
        documento,
        prefijo,
        telefono,
    } = args;
    let tipo_doc_code = tipo_documento.as_deref().map(tipo_documento_to_code).map(str::to_string);

    let mut store = config::load()?;
    match store.profiles.iter_mut().find(|p| p.name == name) {
        Some(p) => {
            if token.is_some() {
                p.token = token;
            }
            if email.is_some() {
                p.email = email;
            }
            if nombre.is_some() {
                p.nombre = nombre;
            }
            if apellido1.is_some() {
                p.apellido1 = apellido1;
            }
            if apellido2.is_some() {
                p.apellido2 = apellido2;
            }
            if tipo_doc_code.is_some() {
                p.tipo_documento = tipo_doc_code;
            }
            if documento.is_some() {
                p.documento = documento;
            }
            if prefijo.is_some() {
                p.prefijo = prefijo;
            }
            if telefono.is_some() {
                p.telefono = telefono;
            }
        }
        None => store.profiles.push(Profile {
            name: name.clone(),
            email,
            token,
            nombre,
            apellido1,
            apellido2,
            tipo_documento: tipo_doc_code,
            documento,
            prefijo,
            telefono,
        }),
    }
    if store.active.is_none() {
        store.active = Some(name.clone());
    }
    config::save(&store)?;
    if !json {
        println!("Perfil «{name}» guardado.");
    }
    Ok(())
}

fn use_profile(name: String, json: bool) -> Result<()> {
    let mut store = config::load()?;
    if !store.profiles.iter().any(|p| p.name == name) {
        bail!("no existe el perfil «{name}»");
    }
    store.active = Some(name.clone());
    config::save(&store)?;
    if !json {
        println!("Perfil activo: «{name}».");
    }
    Ok(())
}

pub fn whoami(json: bool) -> Result<()> {
    let active = config::resolve(None)?;
    match active {
        Some(p) if json => println!("{}", serde_json::to_string_pretty(&p)?),
        Some(p) => println!("{} {}", p.name, p.email.unwrap_or_default()),
        None => bail!("no hay perfil activo"),
    }
    Ok(())
}
