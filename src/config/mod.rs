use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Token/cookie de sesión. Sensible: ver CAPTURA.md.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    // --- Datos del viajero titular para `renfe buy` ----------------------
    // Sensibles: no se logean nunca y solo se inyectan en el POST a Renfe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nombre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apellido1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apellido2: Option<String>,
    /// Código tipo de documento Renfe: "0021" = DNI, "0014" = Pasaporte, "0023" = NIE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tipo_documento: Option<String>,
    /// Número de documento.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documento: Option<String>,
    /// Prefijo telefónico (incluye `+`, p. ej. "+34").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefijo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telefono: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Store {
    pub active: Option<String>,
    pub profiles: Vec<Profile>,
}

/// Directorio de estado local (~/.renfe). Trátalo como sensible.
pub fn dir() -> Result<PathBuf> {
    let base = BaseDirs::new().context("no se pudo resolver el home del usuario")?;
    let d = base.home_dir().join(".renfe");
    fs::create_dir_all(&d).with_context(|| format!("creando {}", d.display()))?;
    Ok(d)
}

fn store_path() -> Result<PathBuf> {
    Ok(dir()?.join("profiles.json"))
}

pub fn load() -> Result<Store> {
    let p = store_path()?;
    if !p.exists() {
        return Ok(Store::default());
    }
    let raw = fs::read_to_string(&p).with_context(|| format!("leyendo {}", p.display()))?;
    Ok(serde_json::from_str(&raw).context("parseando profiles.json")?)
}

pub fn save(store: &Store) -> Result<()> {
    let p = store_path()?;
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(&p, raw).with_context(|| format!("escribiendo {}", p.display()))?;
    Ok(())
}

/// Resuelve el perfil a usar: el indicado por -p, o el activo.
pub fn resolve(name: Option<&str>) -> Result<Option<Profile>> {
    let store = load()?;
    let target = name.map(str::to_string).or(store.active.clone());
    Ok(target.and_then(|t| store.profiles.into_iter().find(|p| p.name == t)))
}
