use crate::api::USER_AGENT;
use crate::models::TrainPosition;
use anyhow::{Context, Result};
use std::time::Duration;

/// Endpoint público sin autenticación que alimenta el visor oficial de Renfe.
/// Solo cubre largo recorrido. El parámetro `v` es un timestamp anti-caché.
const FLEET_URL: &str = "https://infraestructurasferroviarias.renfe.com/visorld/flotaLD.json";

/// Descarga el estado de toda la flota activa de largo recorrido.
///
/// La estructura del JSON la define Renfe y puede cambiar. Se parsea de forma
/// defensiva sobre `serde_json::Value`. AJUSTAR los nombres de campo
/// (`cdgoTren`, `ultRetraso`, lat/lon...) tras inspeccionar una respuesta real.
pub fn fleet() -> Result<Vec<TrainPosition>> {
    let v = chrono::Utc::now().timestamp_millis();
    let url = format!("{FLEET_URL}?v={v}");
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;
    let body: serde_json::Value = client
        .get(&url)
        .send()
        .context("consultando telemetría de flota")?
        .json()
        .context("parseando respuesta de telemetría")?;

    // La carga útil suele ser un array en la raíz o bajo una clave contenedora.
    let arr = body
        .as_array()
        .cloned()
        .or_else(|| {
            body.as_object().and_then(|o| {
                o.values()
                    .find_map(|v| v.as_array().cloned())
            })
        })
        .unwrap_or_default();

    let str_of = |o: &serde_json::Value, keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            o.get(k).and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
        })
    };
    let f64_of = |o: &serde_json::Value, keys: &[&str]| -> Option<f64> {
        keys.iter().find_map(|k| {
            o.get(k).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.replace(',', ".").parse().ok()))
            })
        })
    };

    let mut out = Vec::new();
    for o in arr {
        let train_number = str_of(&o, &["cdgoTren", "numTren", "tren"]).unwrap_or_default();
        let service = str_of(&o, &["tipoTren", "servicio", "tipo"]).unwrap_or_default();
        let lat = f64_of(&o, &["lat", "latitud", "y"]);
        let lon = f64_of(&o, &["lon", "lng", "longitud", "x"]);
        let delay_min = f64_of(&o, &["ultRetraso", "retraso", "delay"])
            .map(|d| d as i64)
            .unwrap_or(0);
        if let (Some(lat), Some(lon)) = (lat, lon) {
            out.push(TrainPosition {
                train_number,
                service,
                lat,
                lon,
                delay_min,
                last_station: str_of(&o, &["estAnt", "estacionAnterior"]),
                next_station: str_of(&o, &["estSig", "estacionSiguiente", "proxParada"]),
            });
        }
    }
    Ok(out)
}
