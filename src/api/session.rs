use crate::api::USER_AGENT;
use crate::config::Profile;
use anyhow::{Context, Result};
use reqwest::cookie::{CookieStore, Jar};
use std::sync::Arc;
use std::time::Duration;

/// Cliente HTTP compartido. Mantiene cookies entre peticiones, que es lo que
/// necesita el flujo de venta de Renfe (sesión basada en JSESSIONID + estado
/// de "negociación" del trayecto entre pasos).
pub struct Session {
    pub http: reqwest::blocking::Client,
    pub profile: Option<Profile>,
    /// Almacén de cookies accesible. El cliente HTTP lo comparte por `Arc`
    /// vía `cookie_provider`, así que cualquier `Set-Cookie` que el servidor
    /// devuelva queda registrado aquí y se puede exportar para `renfe buy`.
    pub jar: Arc<Jar>,
}

impl Session {
    pub fn new(profile: Option<Profile>) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let mut builder = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .timeout(Duration::from_secs(20));
        // Debug: redirigir todo el tráfico por mitmproxy para diagnosticar.
        // Activar con `RENFE_DEBUG_PROXY=1` (o url) y NO usar en producción.
        if let Ok(p) = std::env::var("RENFE_DEBUG_PROXY") {
            let url = if p == "1" {
                "http://127.0.0.1:8080".to_string()
            } else {
                p
            };
            builder = builder
                .proxy(reqwest::Proxy::all(&url).context("proxy de debug inválido")?)
                .danger_accept_invalid_certs(true);
        }
        let http = builder.build().context("construyendo cliente HTTP")?;
        Ok(Self { http, profile, jar })
    }

    /// Inyecta el token de sesión capturado como cookie/cabecera.
    pub fn auth_header(&self) -> Option<(&'static str, String)> {
        self.profile
            .as_ref()
            .and_then(|p| p.token.clone())
            .map(|t| ("Cookie", t))
    }

    /// Devuelve el header `Cookie: ...` que se mandaría al hacer una petición
    /// a `url` (todas las cookies aplicables). `None` si no hay cookies o la
    /// URL no parsea.
    pub fn cookie_header(&self, url: &str) -> Option<String> {
        let parsed: reqwest::Url = url.parse().ok()?;
        self.jar
            .cookies(&parsed)
            .and_then(|h| h.to_str().ok().map(str::to_string))
    }
}
