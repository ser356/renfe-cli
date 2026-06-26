use serde::{Deserialize, Serialize};

/// Estación del catálogo de Renfe.
///
/// Renfe usa dos códigos distintos según el endpoint:
///   * `code` (`cdgoEstacion`) — "60000", "SALAM", "34010". Lo usa el DWR de
///     búsqueda en `origen`/`destino`.
///   * `clave()` — composición `admon,code,uic` ("0071,60000,00600"). Lo usa
///     el form clásico de `buscarTren.do` en `cdgoOrigen`/`cdgoDestino`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    /// `cdgoEstacion` del catálogo (p. ej. "60000" = Madrid Atocha).
    pub code: String,
    /// Nombre legible (`desgEstacion`).
    pub name: String,
    /// `cdgoAdmon` (operador/red). Suele ser "0071" para Renfe.
    #[serde(default)]
    pub admon: String,
    /// `cdgoUic` cuando existe. Algunos alias ("MADRID (TODAS)") no tienen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uic: Option<String>,
}

impl Station {
    /// Composición `admon,code,uic` que espera el form `buscarTren.do`.
    /// Si falta `uic` o `admon`, se sustituye por "null" como hace el frontend.
    pub fn clave(&self) -> String {
        let admon = if self.admon.is_empty() { "null" } else { &self.admon };
        let uic = self.uic.as_deref().unwrap_or("null");
        format!("{admon},{},{uic}", self.code)
    }
}

/// Una opción de tren devuelta por la búsqueda de venta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainOption {
    pub train_type: String,   // AVE, AVLO, Alvia, MD...
    pub train_number: String, // cdgoTren
    pub departure: String,    // HH:MM
    pub arrival: String,      // HH:MM
    pub duration: String,     // HH:MM
    /// Precio mínimo disponible en euros, si lo hay.
    pub price: Option<f64>,
    /// Tarifas disponibles (Básico, Elige, Prémium...).
    pub fares: Vec<Fare>,
    /// Hay al menos una plaza comprable.
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fare {
    pub name: String,
    pub class: String, // Turista, Confort, Turista Plus...
    pub price: f64,
    pub available: bool,
    /// `tpEnlaceSilencio` ("4#18900"): identifica este enlace+tren+tarifa en
    /// los DWR de selección. Sin esto no se puede armar el carrito.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enlace_id: Option<String>,
}

/// Posición/estado en tiempo real de un tren (telemetría sin auth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainPosition {
    pub train_number: String,
    pub service: String, // AVE, Alvia...
    pub lat: f64,
    pub lon: f64,
    /// Retraso acumulado en minutos.
    pub delay_min: i64,
    pub last_station: Option<String>,
    pub next_station: Option<String>,
}
