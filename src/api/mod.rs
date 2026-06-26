pub mod sale;
pub mod session;
pub mod stations;
pub mod telemetry;

/// User-Agent realista. Renfe filtra agentes obviamente automatizados.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
