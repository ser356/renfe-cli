use crate::api::USER_AGENT;
use crate::config;
use crate::models::Station;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::time::Duration;

const CATALOG_URL: &str =
    "https://www.renfe.com/content/dam/renfe/es/General/buscadores/javascript/estacionesEstaticas.js";

fn cache_path() -> Result<std::path::PathBuf> {
    Ok(config::dir()?.join("stations.json"))
}

/// Carga el catálogo: caché local si existe, si no lo descarga.
pub fn load(refresh: bool) -> Result<Vec<Station>> {
    let cache = cache_path()?;
    if !refresh {
        if let Ok(raw) = fs::read_to_string(&cache) {
            if let Ok(list) = serde_json::from_str::<Vec<Station>>(&raw) {
                if !list.is_empty() {
                    return Ok(list);
                }
            }
        }
    }
    let list = fetch_remote()?;
    fs::write(&cache, serde_json::to_string(&list)?)
        .with_context(|| format!("cacheando {}", cache.display()))?;
    Ok(list)
}

fn fetch_remote() -> Result<Vec<Station>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()?;
    // El JS oficial viene en ISO-8859-1; sin esto, Peñaranda/Á/Ñ caen a ?.
    let body = client
        .get(CATALOG_URL)
        .send()
        .context("descargando catálogo de estaciones")?
        .text_with_charset("iso-8859-1")
        .context("leyendo cuerpo del catálogo")?;
    parse_catalog(&body)
}

/// Extrae el array JSON embebido en el JS y lo parsea de forma defensiva.
///
/// El fichero define varias variables (`estacionesEstatico`, `estacionesDestacada`,
/// etc.). Aquí solo nos interesa la primera (catálogo completo). Se localiza
/// y se extrae con un escaneo balanceado de `[`/`]` respetando strings.
fn parse_catalog(js: &str) -> Result<Vec<Station>> {
    let array_str = extract_first_array(js)
        .ok_or_else(|| anyhow!("no se encontró un array de estaciones en el catálogo"))?;
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(array_str).context("parseando array de estaciones")?;

    let name_keys = ["desgEstacion", "desgEstacionPlano", "descEstacion", "nombre", "name"];
    let code_keys = ["cdgoEstacion", "clave", "codigo", "code", "id"];
    let admon_keys = ["cdgoAdmon", "admon"];
    let uic_keys = ["cdgoUic", "uic"];

    let pick_str = |obj: &serde_json::Value, keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            obj.get(k).and_then(|v| {
                v.as_str().filter(|s| !s.is_empty()).map(str::to_string).or_else(|| {
                    v.as_i64().map(|n| n.to_string())
                })
            })
        })
    };

    let mut out = Vec::with_capacity(raw.len());
    for obj in raw {
        let name = pick_str(&obj, &name_keys);
        let code = pick_str(&obj, &code_keys);
        if let (Some(name), Some(code)) = (name, code) {
            let admon = pick_str(&obj, &admon_keys).unwrap_or_default();
            let uic = pick_str(&obj, &uic_keys);
            out.push(Station { code, name, admon, uic });
        }
    }
    if out.is_empty() {
        return Err(anyhow!(
            "catálogo parseado pero vacío: revisar nombres de claves en parse_catalog()"
        ));
    }
    Ok(out)
}

/// Devuelve el primer `[ ... ]` cerrado del texto, respetando strings.
fn extract_first_array(js: &str) -> Option<&str> {
    let bytes = js.as_bytes();
    let start = js.find('[')?;
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == q {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'[' => {
                depth += 1;
                i += 1;
            }
            b']' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(&js[start..i]);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Resuelve un texto de usuario a un único código de estación.
/// Acepta el código exacto o una coincidencia por substring del nombre.
pub fn resolve(query: &str, catalog: &[Station]) -> Result<Station> {
    if let Some(s) = catalog.iter().find(|s| s.code == query) {
        return Ok(s.clone());
    }
    let q = query.to_lowercase();
    let matches: Vec<&Station> = catalog
        .iter()
        .filter(|s| s.name.to_lowercase().contains(&q))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(anyhow!("sin coincidencias para «{query}»")),
        many => {
            let opts = many
                .iter()
                .take(8)
                .map(|s| format!("  {} — {}", s.code, s.name))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!("«{query}» es ambiguo:\n{opts}"))
        }
    }
}
