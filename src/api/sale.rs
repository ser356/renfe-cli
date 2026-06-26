use crate::api::session::Session;
use crate::config::Profile;
use crate::models::{Fare, Station, TrainOption};
use anyhow::{anyhow, bail, Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Parámetros de una búsqueda de venta.
pub struct SearchParams<'a> {
    pub origin: &'a Station,
    pub destination: &'a Station,
    pub date: &'a str, // YYYY-MM-DD
    pub return_date: Option<&'a str>,
    pub adults: u8,
}

const HOME_URL: &str = "https://www.renfe.com/es/es";
const SEARCH_URL: &str = "https://venta.renfe.com/vol/buscarTren.do?Idioma=es&Pais=ES";
const ENLACES_URL: &str = "https://venta.renfe.com/vol/buscarTrenEnlaces.do";
const DATOS_VIAJE_URL: &str = "https://venta.renfe.com/vol/datosViajeEnlaces.do";
const FORMAS_PAGO_URL: &str = "https://venta.renfe.com/vol/formasDePagoEnlaces.do";
const DWR_BASE: &str = "https://venta.renfe.com/vol/dwr/call/plaincall";
const DWR_GET_TRAINS_LIST: &str =
    "https://venta.renfe.com/vol/dwr/call/plaincall/trainEnlacesManager.getTrainsList.dwr";

// === Búsqueda ================================================================

/// Paso 1: búsqueda de trenes con precio y disponibilidad.
///
/// Flujo observado en `capturas/`:
///   1. GET a la home para inicializar cookies de www.renfe.com.
///   2. POST `vol/buscarTren.do` (form-urlencoded) → 302 a `vol/buscarTrenEnlaces.do`.
///   3. GET `vol/buscarTrenEnlaces.do` (HTML, sirve para establecer la sesión
///      Tomcat JSESSIONID en el dominio venta.renfe.com).
///   4. POST DWR `trainEnlacesManager.getTrainsList.dwr` → JS con el listado
///      real en `listadoTrenes[].listviajeViewEnlaceBean[]`.
pub fn search(session: &Session, params: &SearchParams) -> Result<Vec<TrainOption>> {
    let date_ida = format_date_dmy(params.date)
        .with_context(|| format!("fecha de ida «{}» no es YYYY-MM-DD", params.date))?;
    let return_dmy = match params.return_date {
        Some(d) => Some(
            format_date_dmy(d)
                .with_context(|| format!("fecha de vuelta «{d}» no es YYYY-MM-DD"))?,
        ),
        None => None,
    };
    // Cuando solo es ida, fecha de vuelta vacía: Renfe ignora trayecto:I si
    // recibe una fecha de vuelta no vacía y devuelve los dos tramos.
    let date_vuelta = return_dmy.clone().unwrap_or_default();
    let trayecto = if return_dmy.is_some() { "IV" } else { "I" };

    // 1) Inicializa cookies. Si falla la home, no es bloqueante: la lista puede
    //    funcionar igual gracias a las cookies que asigna el siguiente POST.
    let _ = session.http.get(HOME_URL).send();

    // 2) POST form de búsqueda. Reqwest sigue el 302 automáticamente.
    post_buscar_tren(session, params, &date_ida)?;

    // 3) GET enlaces (por si reqwest no encadenó la sesión completa).
    let _ = session.http.get(ENLACES_URL).send();

    // 4) Llamada DWR con el listado real.
    let body = build_get_trains_list_body(params, &date_ida, &date_vuelta, trayecto);
    let dwr_text = dwr_post(
        session,
        DWR_GET_TRAINS_LIST,
        &body,
        "buscarTrenEnlaces.do",
    )?;

    let json = parse_dwr_reply(&dwr_text)
        .context("interpretando la respuesta DWR de getTrainsList")?;

    extract_train_options(&json)
}

/// Tarifas y clases de un tren concreto.
///
/// Renfe devuelve las tarifas (y plazas) embebidas dentro del listado que ya
/// produce `search()`. Esta función mantiene la firma original por simetría:
/// no hace un round-trip extra, basta con leer `TrainOption.fares`.
#[allow(dead_code)]
pub fn fares(_session: &Session, train_id: &str) -> Result<Vec<Fare>> {
    bail!(
        "tarifas no se piden por separado: vienen embebidas en search() \
         dentro de TrainOption.fares. Tren id={train_id}"
    )
}

// === Compra (hasta antes del pago) ===========================================

/// Parámetros de una compra (solo ida, 1 adulto). Pago fuera de alcance.
pub struct BuyParams<'a> {
    pub origin: &'a Station,
    pub destination: &'a Station,
    pub date: &'a str,
    /// `id` del tren a comprar (el que se muestra como "Tren" en `renfe search`).
    pub train_id: i64,
    /// Código de tarifa (p. ej. "VR010"). `None` = la primera disponible.
    pub fare_code: Option<&'a str>,
    /// Datos del titular para el formulario de viajero.
    pub viajero: &'a Profile,
}

/// Resultado de `buy()`: estado final del carrito armado.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuyOutcome {
    /// `idCompra` generado (`_AYrE` y similares). Sin él la sesión no es nada.
    pub id_compra: String,
    /// URL absoluta a abrir en navegador para pagar (Redsys + 3DS).
    pub checkout_url: String,
    /// Header `Cookie:` que enlaza la sesión armada con el navegador. Sensible.
    #[serde(skip_serializing)]
    pub cookies_header: String,
    pub chosen_train: TrainOption,
    pub chosen_fare: Fare,
}

/// Datos del viajero validados (todos obligatorios).
struct ViajeroOk<'a> {
    nombre: &'a str,
    apellido1: &'a str,
    apellido2: &'a str,
    tipo_documento: &'a str,
    documento: &'a str,
    email: &'a str,
    prefijo: &'a str,
    telefono: &'a str,
}

fn validate_viajero<'a>(p: &'a Profile) -> Result<ViajeroOk<'a>> {
    fn need<'a>(o: &'a Option<String>, field: &str) -> Result<&'a str> {
        o.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
            anyhow!(
                "perfil incompleto: falta «{field}». \
                 Configúralo con `renfe profile set <perfil> --{field} …`"
            )
        })
    }
    Ok(ViajeroOk {
        nombre: need(&p.nombre, "nombre")?,
        apellido1: need(&p.apellido1, "apellido1")?,
        apellido2: p.apellido2.as_deref().unwrap_or(""),
        tipo_documento: need(&p.tipo_documento, "tipo-documento")?,
        documento: need(&p.documento, "documento")?,
        email: need(&p.email, "email")?,
        prefijo: p.prefijo.as_deref().filter(|s| !s.is_empty()).unwrap_or("+34"),
        telefono: need(&p.telefono, "telefono")?,
    })
}

/// Arma el carrito hasta el paso de pago.
///
/// Reproduce, en este orden, la secuencia capturada con el flag ★ verificada
/// manualmente con el navegador (ver `capturas/`):
///   1. POST `buscarTren.do` (form clásico) + GET `buscarTrenEnlaces.do?c=<id>`.
///   2. DWR `buyEnlacesManager.actualizaObjetosSesion` para crear el carrito.
///   3. DWR `trainEnlacesManager.getTrainsList` para extraer `tpEnlaceSilencio`.
///   4. DWR `validarTarifasSeleccionadas` + `setTrenEnlaceSeleccionado`.
///   5. GET `datosViajeEnlaces.do?c=<id>` + DWR `validateViajero`.
///   6. POST `datosViajeEnlaces.do` con los datos del viajero.
///   7. La sesión queda en `formasDePagoEnlaces.do`, listo para Redsys.
pub fn buy(session: &Session, params: &BuyParams) -> Result<BuyOutcome> {
    let viajero = validate_viajero(params.viajero)?;
    let id_compra = gen_id_compra();
    let date_dmy = format_date_dmy(params.date)
        .with_context(|| format!("fecha «{}» no es YYYY-MM-DD", params.date))?;

    // 1) Búsqueda: form clásico + página de enlaces con el id de compra.
    let search_for_buy = SearchParams {
        origin: params.origin,
        destination: params.destination,
        date: params.date,
        return_date: None,
        adults: 1,
    };
    let _ = session.http.get(HOME_URL).send();
    post_buscar_tren(session, &search_for_buy, &date_dmy)?;
    let _ = session
        .http
        .get(format!("{ENLACES_URL}?c={id_compra}"))
        .send();

    // 2) Inicializa el carrito en sesión con nuestro idCompra.
    //    El navegador real llama a `actualizaObjetosSesion` DOS veces (antes
    //    y después de un par de DWR de telemetría/login). Repetimos para que
    //    el carrito quede en el mismo estado que el frontend asume.
    dwr_actualiza_objetos_sesion(session, &id_compra)?;
    dwr_actualiza_objetos_sesion(session, &id_compra)?;

    // 3) Lista de trenes (necesitamos el `tpEnlaceSilencio` del elegido).
    let body = build_get_trains_list_body(&search_for_buy, &date_dmy, "", "I");
    let raw = dwr_post(session, DWR_GET_TRAINS_LIST, &body, "buscarTrenEnlaces.do")?;
    let json =
        parse_dwr_reply(&raw).context("interpretando getTrainsList durante buy()")?;
    let trains = extract_train_options(&json)?;

    let chosen = trains
        .iter()
        .find(|t| t.train_number.parse::<i64>().ok() == Some(params.train_id))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "tren id={} no está en la lista (hay {} trenes). \
                 Ejecuta `renfe search` para ver los ids disponibles.",
                params.train_id,
                trains.len()
            )
        })?;
    let fare = match params.fare_code {
        Some(code) => chosen
            .fares
            .iter()
            .find(|f| f.name == code)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "tarifa «{code}» no disponible para el tren {}. \
                     Disponibles: {}",
                    params.train_id,
                    chosen
                        .fares
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
        None => chosen.fares.first().cloned().ok_or_else(|| {
            anyhow!("el tren {} no tiene tarifas disponibles", params.train_id)
        })?,
    };
    let enlace_id = fare.enlace_id.as_deref().ok_or_else(|| {
        anyhow!(
            "tarifa «{}» sin `tpEnlaceSilencio`. ¿Cambió el esquema de Renfe?",
            fare.name
        )
    })?;

    // 4) Validación + selección efectiva del enlace.
    dwr_validar_tarifas(session, &id_compra, params.train_id, &fare.name, enlace_id)?;
    dwr_set_tren_enlace(session, &id_compra, params.train_id, &fare.name, enlace_id)?;

    // 5) Pantalla de datos del viajero + validación DWR.
    let _ = session
        .http
        .get(format!("{DATOS_VIAJE_URL}?c={id_compra}"))
        .send();
    dwr_validate_viajero(session, &id_compra, &viajero, &fare.name)?;

    // 6) Submit definitivo del formulario de viajero. Sigue redirects hasta
    //    formasDePagoEnlaces.do; si Renfe no acepta, sale aquí.
    post_datos_viaje(session, &id_compra, &viajero, &fare.name)?;

    // 7) Outcome.
    let checkout_url = format!("{FORMAS_PAGO_URL}?c={id_compra}");
    let cookies_header = session
        .cookie_header(&checkout_url)
        .unwrap_or_default();

    Ok(BuyOutcome {
        id_compra,
        checkout_url,
        cookies_header,
        chosen_train: chosen,
        chosen_fare: fare,
    })
}

// === Construcción del body DWR ===============================================

/// Construye el cuerpo plain-text del DWR `trainEnlacesManager.getTrainsList`.
fn build_get_trains_list_body(
    params: &SearchParams,
    date_ida: &str,
    date_vuelta: &str,
    trayecto: &str,
) -> String {
    let ida_enc = enc_slashes(date_ida);
    let vuelta_enc = enc_slashes(date_vuelta);
    let session_id = build_script_session_id();
    let adults = params.adults.to_string();

    let mut b = String::with_capacity(1024);
    b.push_str("callCount=1\n");
    b.push_str("windowName=\n");
    b.push_str("c0-scriptName=trainEnlacesManager\n");
    b.push_str("c0-methodName=getTrainsList\n");
    b.push_str("c0-id=0\n");
    b.push_str("c0-e1=string:false\n");
    b.push_str("c0-e2=string:false\n");
    b.push_str("c0-e3=string:false\n");
    b.push_str("c0-e4=string:\n");
    b.push_str("c0-e5=string:\n");
    b.push_str("c0-e6=string:\n");
    b.push_str("c0-e7=string:\n");
    b.push_str(&format!("c0-e8=string:{ida_enc}\n"));
    b.push_str(&format!("c0-e9=string:{vuelta_enc}\n"));
    b.push_str(&format!("c0-e10=string:{adults}\n"));
    b.push_str("c0-e11=string:0\n");
    b.push_str("c0-e12=string:0\n");
    b.push_str(&format!("c0-e13=string:{trayecto}\n"));
    b.push_str("c0-e14=string:\n");
    b.push_str("c0-e15=string:false\n");
    b.push_str("c0-e16=string:false\n");
    b.push_str(&format!(
        "c0-e17=string:{}\n",
        enc_dwr_value(&params.origin.code)
    ));
    b.push_str(&format!(
        "c0-e18=string:{}\n",
        enc_dwr_value(&params.destination.code)
    ));
    b.push_str("c0-e19=string:\n");
    b.push_str(
        "c0-param0=Object_Object:{atendo:reference:c0-e1, sinEnlace:reference:c0-e2, \
         plazaH:reference:c0-e3, tipoFranjaI:reference:c0-e4, tipoFranjaV:reference:c0-e5, \
         horaFranjaIda:reference:c0-e6, horaFranjaVuelta:reference:c0-e7, \
         fechaSalida:reference:c0-e8, fechaVuelta:reference:c0-e9, \
         adultos:reference:c0-e10, ninos:reference:c0-e11, ninosMenores:reference:c0-e12, \
         trayecto:reference:c0-e13, idaVuelta:reference:c0-e14, \
         conMascota:reference:c0-e15, conBicicleta:reference:c0-e16, \
         origen:reference:c0-e17, destino:reference:c0-e18, codPromo:reference:c0-e19}\n",
    );
    b.push_str("batchId=1\n");
    b.push_str("instanceId=0\n");
    b.push_str("page=%2Fvol%2FbuscarTrenEnlaces.do\n");
    b.push_str(&format!("scriptSessionId={session_id}\n"));
    b
}

// === Parser de respuestas DWR ================================================

/// Transforma la respuesta DWR (`r.handleCallback("...","...",{...});`) en JSON
/// estándar y la parsea.
///
/// La respuesta DWR es JavaScript, no JSON: trae `new Date(N)`, comillas
/// simples y, sobre todo, claves de objeto sin entrecomillar. Aquí se hace un
/// mini-tokenizado que respeta strings y reemplaza:
///   - `new Date(N)` → `N` (timestamp en ms).
///   - `clave:` → `"clave":` (cuando es una key real, no parte de una string).
pub fn parse_dwr_reply(body: &str) -> Result<serde_json::Value> {
    let obj_js = extract_handle_callback_object(body)
        .ok_or_else(|| anyhow!("respuesta DWR sin handleCallback parseable"))?;
    let json = dwr_object_to_json(obj_js);
    serde_json::from_str(&json).context("convertir objeto DWR a JSON")
}

/// Devuelve el slice con el objeto JS que llega como tercer argumento de
/// `r.handleCallback("X","Y", {...})`.
fn extract_handle_callback_object(body: &str) -> Option<&str> {
    let key = "handleCallback(";
    let mut start = body.find(key)? + key.len();
    let bytes = body.as_bytes();
    let mut commas = 0;
    while start < bytes.len() && commas < 2 {
        match bytes[start] {
            b'"' | b'\'' => {
                let q = bytes[start];
                start += 1;
                while start < bytes.len() {
                    if bytes[start] == b'\\' {
                        start += 2;
                        continue;
                    }
                    if bytes[start] == q {
                        start += 1;
                        break;
                    }
                    start += 1;
                }
            }
            b',' => {
                commas += 1;
                start += 1;
            }
            _ => start += 1,
        }
    }
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }
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
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(&body[start..i]);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Transforma un object literal JS al subset compatible con JSON.
fn dwr_object_to_json(js: &str) -> String {
    let bytes = js.as_bytes();
    let mut out = String::with_capacity(js.len() + js.len() / 8);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let q = c;
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            if q == b'\'' {
                out.push('"');
                out.push_str(&js[start + 1..i - 1].replace('"', "\\\""));
                out.push('"');
            } else {
                out.push_str(&js[start..i]);
            }
            continue;
        }
        if c == b'n' && js[i..].starts_with("new Date(") {
            let after = i + "new Date(".len();
            let mut j = after;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'-') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b')' {
                out.push_str(&js[after..j]);
                i = j + 1;
                continue;
            }
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            let ident = &js[start..i];
            let mut k = i;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            let is_key = k < bytes.len() && bytes[k] == b':';
            let reserved = matches!(ident, "true" | "false" | "null");
            if is_key && !reserved {
                out.push('"');
                out.push_str(ident);
                out.push('"');
            } else {
                out.push_str(ident);
            }
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

// === Mapeo del JSON a TrainOption ===========================================

/// Aplana `listadoTrenes[].listviajeViewEnlaceBean[]` en una lista única de
/// `TrainOption`. Tolera tanto el caso de un solo tramo como ida+vuelta.
fn extract_train_options(json: &serde_json::Value) -> Result<Vec<TrainOption>> {
    let tramos = json
        .get("listadoTrenes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("respuesta sin 'listadoTrenes'"))?;

    let mut out = Vec::new();
    for tramo in tramos {
        let viajes = match tramo.get("listviajeViewEnlaceBean").and_then(|v| v.as_array()) {
            Some(v) => v,
            None => continue,
        };
        for v in viajes {
            out.push(map_viaje(v));
        }
    }
    Ok(out)
}

fn map_viaje(v: &serde_json::Value) -> TrainOption {
    let str_of = |keys: &[&str]| -> String {
        keys.iter()
            .find_map(|k| v.get(k).and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let bool_of = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);

    let train_type = str_of(&["tipoTrenUno", "claseTipoTren", "cdgoOperador"]);
    let train_number = v
        .get("id")
        .and_then(|x| x.as_i64())
        .map(|n| n.to_string())
        .unwrap_or_default();

    let departure = str_of(&["horaSalida"]);
    let arrival = str_of(&["horaLlegada"]);
    let duration = format_duration(v);

    let price = v
        .get("tarifaMinima")
        .and_then(|x| x.as_str())
        .and_then(parse_eu_decimal);

    let fares = extract_fares(v);
    let completo = bool_of("completo");
    let plaza_b = bool_of("plazaBDisponible");
    let plaza_h = bool_of("plazaHDisponible");
    let available = !completo && (plaza_b || plaza_h);

    TrainOption {
        train_type,
        train_number,
        departure,
        arrival,
        duration,
        price,
        fares,
        available,
    }
}

fn extract_fares(v: &serde_json::Value) -> Vec<Fare> {
    let Some(arr) = v.get("tarifasDisponibles").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|t| {
            let name = t
                .get("codigoTarifa")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let class = t
                .get("cdgoClase")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let price = t
                .get("precioTarifa")
                .and_then(|x| x.as_str())
                .and_then(parse_eu_decimal)
                .unwrap_or(0.0);
            let available = t
                .get("plazaH")
                .and_then(|x| x.as_bool())
                .or_else(|| {
                    t.get("plazasLibresTren")
                        .and_then(|x| x.as_array())
                        .map(|a| !a.is_empty())
                })
                .unwrap_or(true);
            let enlace_id = t
                .get("tpEnlaceSilencio")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            if name.is_empty() && class.is_empty() && price == 0.0 {
                return None;
            }
            Some(Fare {
                name,
                class,
                price,
                available,
                enlace_id,
            })
        })
        .collect()
}

// === Helpers ================================================================

fn format_date_dmy(yyyy_mm_dd: &str) -> Result<String> {
    let parts: Vec<&str> = yyyy_mm_dd.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        bail!("fecha no es YYYY-MM-DD: {yyyy_mm_dd}");
    }
    for p in &parts {
        if !p.bytes().all(|b| b.is_ascii_digit()) {
            bail!("fecha no es YYYY-MM-DD: {yyyy_mm_dd}");
        }
    }
    Ok(format!("{}/{}/{}", parts[2], parts[1], parts[0]))
}

fn enc_slashes(s: &str) -> String {
    s.replace('/', "%2F")
}

fn enc_dwr_value(s: &str) -> String {
    s.replace('\r', "").replace('\n', "")
}

fn build_script_session_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("{ts}/{ts}-RNF")
}

fn format_duration(v: &serde_json::Value) -> String {
    if let Some(mins) = v.get("duracionViajeTotalEnMinutos").and_then(|x| x.as_i64()) {
        if mins > 0 {
            return format!("{:02}:{:02}", mins / 60, mins % 60);
        }
    }
    v.get("duracionViaje")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn parse_eu_decimal(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok()
}

// === Reutilizables HTTP/DWR =================================================

/// POST al form clásico de búsqueda. Lo usan `search()` y `buy()`.
fn post_buscar_tren(
    session: &Session,
    params: &SearchParams,
    date_ida: &str,
) -> Result<()> {
    let adults = params.adults.to_string();
    let clave_origen = params.origin.clave();
    let clave_destino = params.destination.clave();
    let date_vuelta = match params.return_date {
        Some(d) => format_date_dmy(d)?,
        None => String::new(),
    };
    let form: Vec<(&str, &str)> = vec![
        ("tipoBusqueda", "autocomplete"),
        ("currenLocation", "menuBusqueda"),
        ("vengoderenfecom", "SI"),
        ("desOrigen", &params.origin.name),
        ("desDestino", &params.destination.name),
        ("cdgoOrigen", &clave_origen),
        ("cdgoDestino", &clave_destino),
        ("idiomaBusqueda", "ES"),
        ("FechaIdaSel", date_ida),
        ("FechaVueltaSel", &date_vuelta),
        ("_fechaIdaVisual", date_ida),
        ("_fechaVueltaVisual", &date_vuelta),
        ("minPriceDeparture", "false"),
        ("minPriceReturn", "false"),
        ("adultos_", &adults),
        ("ninos_", "0"),
        ("ninosMenores", "0"),
        ("codPromocional", ""),
        ("plazaH", "false"),
        ("sinEnlace", "false"),
        ("conMascota", "false"),
        ("conBicicleta", "false"),
        ("asistencia", "false"),
        ("franjaHoraI", ""),
        ("franjaHoraV", ""),
        ("Idioma", "es"),
        ("Pais", "ES"),
    ];

    let mut req = session
        .http
        .post(SEARCH_URL)
        .header("Origin", "https://www.renfe.com")
        .header("Referer", "https://www.renfe.com/")
        .form(&form);
    if let Some((k, v)) = session.auth_header() {
        req = req.header(k, v);
    }
    let resp = req.send().context("POST a buscarTren.do")?;
    if !resp.status().is_success() {
        bail!(
            "buscarTren.do respondió {}: la búsqueda no pasó el formulario inicial",
            resp.status()
        );
    }
    let _ = resp.bytes();
    Ok(())
}

/// Llamada DWR genérica: POST text/plain al script DWR indicado.
fn dwr_post(
    session: &Session,
    url: &str,
    body: &str,
    referer_page: &str,
) -> Result<String> {
    let referer = format!("https://venta.renfe.com/vol/{referer_page}");
    let resp = session
        .http
        .post(url)
        .header("Content-Type", "text/plain")
        .header("Accept", "*/*")
        .header("Origin", "https://venta.renfe.com")
        .header("Referer", referer)
        .body(body.to_string())
        .send()
        .with_context(|| format!("POST DWR {url}"))?;
    if !resp.status().is_success() {
        bail!("DWR {url} respondió {}", resp.status());
    }
    resp.text().context("leyendo cuerpo DWR")
}

/// Inicializa el carrito de venta en la sesión con nuestro `idCompra`. Sin
/// esta llamada, los DWR posteriores no encuentran el objeto compra.
fn dwr_actualiza_objetos_sesion(session: &Session, id_compra: &str) -> Result<()> {
    let session_id = build_script_session_id();
    let mut b = String::new();
    b.push_str("callCount=1\n");
    b.push_str("windowName=\n");
    b.push_str("c0-scriptName=buyEnlacesManager\n");
    b.push_str("c0-methodName=actualizaObjetosSesion\n");
    b.push_str("c0-id=0\n");
    b.push_str(&format!("c0-e1=string:{id_compra}\n"));
    b.push_str("c0-e2=string:\n");
    b.push_str("c0-param0=array:[reference:c0-e1,reference:c0-e2]\n");
    b.push_str("batchId=2\n");
    b.push_str("instanceId=0\n");
    b.push_str(&format!(
        "page=%2Fvol%2FbuscarTrenEnlaces.do%3Fc%3D{}\n",
        id_compra
    ));
    b.push_str(&format!("scriptSessionId={session_id}\n"));

    dwr_post(
        session,
        &format!("{DWR_BASE}/buyEnlacesManager.actualizaObjetosSesion.dwr"),
        &b,
        &format!("buscarTrenEnlaces.do?c={id_compra}"),
    )?;
    Ok(())
}

/// Cuerpo común para `validarTarifasSeleccionadas` y `setTrenEnlaceSeleccionado`:
/// los dos toman exactamente el mismo Object_Object{ida, idCompra, vuelta}.
/// Aquí siempre `vuelta=[]` porque `buy()` es solo ida.
fn build_seleccion_body(
    method: &str,
    id_compra: &str,
    train_id: i64,
    fare_code: &str,
    enlace_id: &str,
    batch: u32,
) -> String {
    let session_id = build_script_session_id();
    let mut b = String::new();
    b.push_str("callCount=1\n");
    b.push_str("windowName=\n");
    b.push_str("c0-scriptName=trainEnlacesManager\n");
    b.push_str(&format!("c0-methodName={method}\n"));
    b.push_str("c0-id=0\n");
    b.push_str(&format!("c0-e2=number:{train_id}\n"));
    b.push_str(&format!("c0-e3=string:{fare_code}\n"));
    b.push_str("c0-e4=boolean:false\n");
    b.push_str(&format!(
        "c0-e5=string:{}\n",
        enc_dwr_value(enlace_id)
    ));
    b.push_str("c0-e1=array:[reference:c0-e2,reference:c0-e3,reference:c0-e4,reference:c0-e5]\n");
    b.push_str(&format!("c0-e6=string:{id_compra}\n"));
    b.push_str("c0-e7=null:null\n");
    b.push_str(
        "c0-param0=Object_Object:{ida:reference:c0-e1, idCompra:reference:c0-e6, \
         vuelta:reference:c0-e7}\n",
    );
    b.push_str(&format!("batchId={batch}\n"));
    b.push_str("instanceId=0\n");
    b.push_str(&format!(
        "page=%2Fvol%2FbuscarTrenEnlaces.do%3Fc%3D{}\n",
        id_compra
    ));
    b.push_str(&format!("scriptSessionId={session_id}\n"));
    b
}

fn dwr_validar_tarifas(
    session: &Session,
    id_compra: &str,
    train_id: i64,
    fare_code: &str,
    enlace_id: &str,
) -> Result<()> {
    let body = build_seleccion_body(
        "validarTarifasSeleccionadas",
        id_compra,
        train_id,
        fare_code,
        enlace_id,
        9,
    );
    dwr_post(
        session,
        &format!("{DWR_BASE}/trainEnlacesManager.validarTarifasSeleccionadas.dwr"),
        &body,
        &format!("buscarTrenEnlaces.do?c={id_compra}"),
    )?;
    Ok(())
}

fn dwr_set_tren_enlace(
    session: &Session,
    id_compra: &str,
    train_id: i64,
    fare_code: &str,
    enlace_id: &str,
) -> Result<()> {
    let body = build_seleccion_body(
        "setTrenEnlaceSeleccionado",
        id_compra,
        train_id,
        fare_code,
        enlace_id,
        10,
    );
    dwr_post(
        session,
        &format!("{DWR_BASE}/trainEnlacesManager.setTrenEnlaceSeleccionado.dwr"),
        &body,
        &format!("buscarTrenEnlaces.do?c={id_compra}"),
    )?;
    Ok(())
}

/// Encode mínimo URL-safe para meter en el body DWR. `+` se conserva como
/// literal; `/` y `@` y `&` se entrecomillan porque romperían `key=val`.
fn dwr_url_enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push_str("%20"),
            '+' => out.push_str("%2B"),
            '/' => out.push_str("%2F"),
            '@' => out.push_str("%40"),
            ':' => out.push_str("%3A"),
            ',' => out.push_str("%2C"),
            '&' => out.push_str("%26"),
            '=' => out.push_str("%3D"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            other => {
                let mut buf = [0u8; 4];
                for b in other.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

fn dwr_validate_viajero(
    session: &Session,
    id_compra: &str,
    v: &ViajeroOk<'_>,
    fare_code: &str,
) -> Result<()> {
    let session_id = build_script_session_id();
    let mut b = String::new();
    b.push_str("callCount=1\n");
    b.push_str("windowName=\n");
    b.push_str("c0-scriptName=travelData\n");
    b.push_str("c0-methodName=validateViajero\n");
    b.push_str("c0-id=0\n");
    b.push_str(&format!("c0-e2=string:{}\n", dwr_url_enc(v.nombre)));
    b.push_str(&format!("c0-e3=string:{}\n", dwr_url_enc(v.apellido1)));
    b.push_str(&format!("c0-e4=string:{}\n", dwr_url_enc(v.apellido2)));
    b.push_str(&format!("c0-e5=string:{}\n", v.tipo_documento));
    b.push_str(&format!("c0-e6=string:{}\n", dwr_url_enc(v.documento)));
    b.push_str(&format!("c0-e7=string:{}\n", dwr_url_enc(v.email)));
    b.push_str(&format!("c0-e8=string:{}\n", dwr_url_enc(v.prefijo)));
    b.push_str(&format!("c0-e9=string:{}\n", dwr_url_enc(v.telefono)));
    b.push_str("c0-e10=string:A\n");
    b.push_str("c0-e11=null:null\n");
    b.push_str("c0-e12=array:[]\n");
    b.push_str("c0-e15=array:[]\n");
    b.push_str(&format!("c0-e16=string:{fare_code}\n"));
    b.push_str(
        "c0-e14=Object_Object:{listDocSeleccionado:reference:c0-e15, \
         cdgoTarifa:reference:c0-e16}\n",
    );
    b.push_str("c0-e13=array:[reference:c0-e14]\n");
    b.push_str(
        "c0-e1=Object_Object:{nombre:reference:c0-e2, apellido1:reference:c0-e3, \
         apellido2:reference:c0-e4, tipoDocumento:reference:c0-e5, \
         documento:reference:c0-e6, email:reference:c0-e7, \
         prefijo:reference:c0-e8, telefono:reference:c0-e9, \
         tipoViajero:reference:c0-e10, tarjetMasRenfe:reference:c0-e11, \
         atendo:reference:c0-e12, tarifasTrayectos:reference:c0-e13}\n",
    );
    b.push_str("c0-param0=array:[reference:c0-e1]\n");
    b.push_str(&format!("c0-param1=string:{id_compra}\n"));
    b.push_str("batchId=17\n");
    b.push_str("instanceId=0\n");
    b.push_str(&format!(
        "page=%2Fvol%2FdatosViajeEnlaces.do%3Fc%3D{}\n",
        id_compra
    ));
    b.push_str(&format!("scriptSessionId={session_id}\n"));

    let resp = dwr_post(
        session,
        &format!("{DWR_BASE}/travelData.validateViajero.dwr"),
        &b,
        &format!("datosViajeEnlaces.do?c={id_compra}"),
    )?;
    // La respuesta es siempre handleCallback(.., {} ) en éxito; si Renfe
    // devuelve un objeto con `mensaje*` algo no le ha gustado del viajero.
    if resp.contains("mensajeError") || resp.contains("\"error\"") {
        bail!("Renfe rechazó los datos del viajero (validateViajero): {resp}");
    }
    Ok(())
}

/// Submit del formulario clásico de datos del viajero. Reqwest sigue redirects.
///
/// El body replica el capturado, adaptado a solo ida (sin `trayectos[1].*` ni
/// `directoVuelta`). Faltaba `trayectos[0].viajeros[0].tarifa.cdgoTarifa` y
/// el backend devolvía 500 — Renfe necesita explícitamente la tarifa del
/// viajero del trayecto, no le basta con la selección DWR previa.
fn post_datos_viaje(
    session: &Session,
    id_compra: &str,
    v: &ViajeroOk<'_>,
    fare_code: &str,
) -> Result<()> {
    let form: Vec<(&str, &str)> = vec![
        ("compraActual", id_compra),
        ("compraAntigua", id_compra),
        ("nviajeros", "1"),
        ("cambioPrecios", "false"),
        ("ferroviario", ""),
        ("directoIda", "true"),
        ("idaVuelta", "false"),
        ("obligaNombre", "true"),
        // Datos del titular.
        ("formBean.listaViajeros[0].nombre", v.nombre),
        ("formBean.listaViajeros[0].apellido1", v.apellido1),
        ("formBean.listaViajeros[0].apellido2", v.apellido2),
        ("formBean.listaViajeros[0].tipoDocumento", v.tipo_documento),
        ("formBean.listaViajeros[0].documento", v.documento),
        ("formBean.listaViajeros[0].email", v.email),
        ("formBean.listaViajeros[0].prefijo", v.prefijo),
        ("formBean.listaViajeros[0].telefono", v.telefono),
        // Bloque familia numerosa vacío (C070 = sin tarjeta de familia numerosa).
        (
            "formBean.listaViajeros[0].familiaNumerosa.tipoTarjetaFamNumerosa",
            "C070",
        ),
        (
            "formBean.listaViajeros[0].familiaNumerosa.comAutonomaFamNumerosa",
            "",
        ),
        (
            "formBean.listaViajeros[0].familiaNumerosa.fechaNacimientoFamNumerosa",
            "",
        ),
        (
            "formBean.listaViajeros[0].familiaNumerosa.documentoFamNumerosa",
            "",
        ),
        // Tarifa del trayecto 0 (lo crítico): si no se manda, el server 500.
        ("trayectos[0].viajeros[0].tarifa.descuento", fare_code),
        ("trayectos[0].viajeros[0].tarifa.cdgoTarifa", fare_code),
        // Documento adicional (IVDE = sin documento adicional).
        (
            "formBean.listaViajeros[0].tarifasTrayectos[0].tarifasDisponibles.listDocViewBean[3].cdgoDocum",
            "",
        ),
        (
            "formBean.listaViajeros[0].tarifasTrayectos[0].tarifasDisponibles.listDocViewBean[3].cdgoTipoDoc",
            "IVDE",
        ),
        (
            "formBean.listaViajeros[0].tarifasTrayectos[0].tarifasDisponibles.listDocViewBean[3].cdgoTipoOrg",
            "",
        ),
        (
            "formBean.listaViajeros[0].tarifasTrayectos[0].tarifasDisponibles.listDocViewBean[3].isAdicional",
            "true",
        ),
        // Tarifa metadata por trayecto, replicada tal cual del navegador. El
        // server ignora los literales `${indexTrayecto.index}` (son plantillas
        // JSP sin renderizar), pero los esperaba en la captura, así que los
        // mandamos.
        (
            "formBean.listaViajeros['${indexTrayecto.index}'].tarifa.cdgoClaseControl",
            "",
        ),
        (
            "formBean.listaViajeros['${indexTrayecto.index}'].tarifa.cdgoClaseComercial",
            "",
        ),
        (
            "formBean.listaViajeros['${indexTrayecto.index}'].tarifa.descTarifa",
            "-Adulto IDA-",
        ),
        (
            "formBean.listaViajeros['${indexTrayecto.index}'].tipoViajero",
            "A",
        ),
        ("request-assistance-1-0", ""),
    ];
    let resp = session
        .http
        .post(DATOS_VIAJE_URL)
        .header("Origin", "https://venta.renfe.com")
        .header(
            "Referer",
            format!("{DATOS_VIAJE_URL}?c={id_compra}"),
        )
        .form(&form)
        .send()
        .context("POST datosViajeEnlaces.do")?;
    let status = resp.status();
    if !status.is_success() {
        bail!(
            "datosViajeEnlaces.do respondió {}: el carrito no se confirmó. \
             ¿Caducó la sesión o se rechazaron los datos?",
            status
        );
    }
    let _ = resp.bytes();
    Ok(())
}

/// Genera un `idCompra` plausible (`_` + 4 chars alfanuméricos).
/// Renfe lo trata como una clave opaca de la sesión de compra; basta con
/// que sea único por ejecución.
fn gen_id_compra() -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut s = String::from("_");
    for _ in 0..4 {
        s.push(ALPHA[(n % ALPHA.len() as u128) as usize] as char);
        n /= ALPHA.len() as u128;
    }
    s
}

// === Tests ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fecha_dmy() {
        assert_eq!(format_date_dmy("2026-06-26").unwrap(), "26/06/2026");
        assert!(format_date_dmy("26/06/2026").is_err());
        assert!(format_date_dmy("2026-6-26").is_err());
    }

    #[test]
    fn decimal_europeo() {
        assert_eq!(parse_eu_decimal("5,25"), Some(5.25));
        assert_eq!(parse_eu_decimal("4,2"), Some(4.2));
        assert_eq!(parse_eu_decimal("12.34"), Some(12.34));
        assert!(parse_eu_decimal("nope").is_none());
    }

    #[test]
    fn dwr_object_quotea_keys_y_normaliza_date() {
        let js = "{a:1,b:\"x:y\",c:new Date(1782452100000),d:true}";
        let json = dwr_object_to_json(js);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], "x:y");
        assert_eq!(v["c"], 1782452100000_i64);
        assert_eq!(v["d"], true);
    }

    #[test]
    fn parse_dwr_reply_sintetico() {
        let body = "throw 'allowScriptTagRemoting is false.';\n\
                    (function(){\nvar r=window.dwr._[0];\n\
                    r.handleCallback(\"1\",\"0\",{listadoTrenes:[{listviajeViewEnlaceBean:[\
                    {id:1,horaSalida:\"05:47\",horaLlegada:\"06:06\",\
                    duracionViaje:\"0 horas 19 minutos\",duracionViajeTotalEnMinutos:19,\
                    tipoTrenUno:\"MD\",tarifaMinima:\"4,2\",completo:false,\
                    plazaBDisponible:true,plazaHDisponible:true,\
                    tarifasDisponibles:[{codigoTarifa:\"VR010\",cdgoClase:\"T\",\
                    precioTarifa:\"5,25\",plazaH:true,tpEnlaceSilencio:\"4#18900\"}]}]}]});\n})();\n";
        let v = parse_dwr_reply(body).unwrap();
        let trenes = extract_train_options(&v).unwrap();
        assert_eq!(trenes.len(), 1);
        let t = &trenes[0];
        assert_eq!(t.train_type, "MD");
        assert_eq!(t.train_number, "1");
        assert_eq!(t.departure, "05:47");
        assert_eq!(t.arrival, "06:06");
        assert_eq!(t.duration, "00:19");
        assert_eq!(t.price, Some(4.2));
        assert!(t.available);
        assert_eq!(t.fares.len(), 1);
        assert_eq!(t.fares[0].name, "VR010");
        assert_eq!(t.fares[0].price, 5.25);
        assert_eq!(t.fares[0].enlace_id.as_deref(), Some("4#18900"));
    }
}
