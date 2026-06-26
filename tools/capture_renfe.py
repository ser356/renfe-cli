"""
capture_renfe.py — addon de mitmproxy para reconstruir la API de venta de Renfe.

Qué hace
--------
Mientras navegas y haces una compra real en venta.renfe.com (hasta justo antes
de pagar), este addon:

  * Captura SOLO lo relevante de los dominios de Renfe. Ignora estáticos
    (.js/.css/imágenes/fuentes) y analítica de terceros.
  * Vuelca cada petición/respuesta a ./capturas/NNN_<metodo>_<path>.json con
    cabeceras significativas, cuerpo y respuesta (JSON formateado si aplica).
  * Mantiene ./capturas/index.jsonl con un resumen por flujo.
  * Resalta EN VIVO los flujos que parecen traer trenes/precios/tarifas, para
    que vayas directo al endpoint de búsqueda sin bucear en decenas de XHR.
  * Para cada flujo interesante genera un .rs de referencia con el equivalente
    en `reqwest`, listo para adaptar en src/api/sale.rs.

Uso
---
    pip install mitmproxy            # si no lo tienes (o brew install mitmproxy)
    mitmweb -s tools/capture_renfe.py     # UI en http://127.0.0.1:8081
    # o sin interfaz:
    mitmdump -s tools/capture_renfe.py

Configura el proxy del navegador/sistema a 127.0.0.1:8080 e instala el
certificado de mitmproxy (http://mitm.it) para ver el tráfico TLS.

Aviso
-----
Los volcados contienen TUS cookies y token de sesión en claro. La carpeta
./capturas/ es tan sensible como ~/.renfe/. No la subas al repo.
"""

import json
import re
from pathlib import Path

from mitmproxy import http, ctx

# --- Configuración -----------------------------------------------------------

OUT_DIR = Path("capturas")

# Hosts cuyo tráfico nos interesa. Cualquier host que contenga "renfe" entra,
# salvo los de la lista de ruido de abajo.
RENFE_HINT = "renfe"

# Ruido a ignorar aunque el host contenga "renfe" o venga del mismo origen.
IGNORE_HOSTS = (
    "google", "googletagmanager", "google-analytics", "doubleclick",
    "facebook", "hotjar", "cookiebot", "onetrust", "cdn", "cloudflareinsights",
    "adservice", "newrelic", "nr-data", "clarity.ms", "evidon",
)

# Extensiones de recursos estáticos que no aportan al flujo de API.
IGNORE_EXT = re.compile(
    r"\.(js|css|png|jpe?g|gif|svg|webp|woff2?|ttf|eot|ico|map|mp4|webm)(\?|$)",
    re.IGNORECASE,
)

# Palabras que, en una respuesta JSON, sugieren que es el endpoint de venta.
INTEREST_KEYS = (
    "tren", "trenes", "horario", "tarifa", "precio", "plaza", "salida",
    "llegada", "ave", "avlo", "trayecto", "listaTren", "disponib",
)

# Cabeceras de petición que merece la pena conservar (el resto es ruido).
KEEP_REQ_HEADERS = (
    "content-type", "accept", "referer", "origin", "x-requested-with",
    "cookie", "authorization", "x-csrf-token", "x-xsrf-token",
)

# --- Estado ------------------------------------------------------------------

_counter = 0


def load(loader):
    OUT_DIR.mkdir(exist_ok=True)
    ctx.log.info(f"[renfe] capturando en ./{OUT_DIR}/  (Ctrl-C para parar)")


def _is_target(flow: http.HTTPFlow) -> bool:
    host = flow.request.pretty_host.lower()
    if RENFE_HINT not in host:
        return False
    if any(bad in host for bad in IGNORE_HOSTS):
        return False
    if IGNORE_EXT.search(flow.request.path):
        return False
    return True


def _pretty(body: bytes):
    """Devuelve (texto, es_json). Formatea JSON si puede."""
    if not body:
        return "", False
    try:
        text = body.decode("utf-8", "replace")
    except Exception:
        return f"<{len(body)} bytes binarios>", False
    try:
        return json.dumps(json.loads(text), ensure_ascii=False, indent=2), True
    except Exception:
        return text, False


def _filter_headers(headers, keep):
    return {k: v for k, v in headers.items() if k.lower() in keep}


def _looks_interesting(resp_text: str, is_json: bool) -> bool:
    if not is_json:
        return False
    low = resp_text.lower()
    hits = sum(1 for k in INTEREST_KEYS if k in low)
    return hits >= 3  # varias señales a la vez para evitar falsos positivos


def _slug(path: str) -> str:
    path = path.split("?")[0].strip("/")
    path = re.sub(r"[^A-Za-z0-9_-]+", "_", path) or "root"
    return path[:60]


def _rust_snippet(flow: http.HTTPFlow, req_body_text: str, is_json_req: bool) -> str:
    """Genera un equivalente reqwest de referencia para pegar en sale.rs."""
    method = flow.request.method.lower()
    url = flow.request.url.split("?")[0]
    query = dict(flow.request.query)

    lines = [
        "// Referencia autogenerada por capture_renfe.py — ADAPTAR, no copiar tal cual.",
        "// Reutiliza session.http (mantiene cookies). No cablees la Cookie a mano:",
        "// inyéctala vía perfil/Session::auth_header().",
        "",
        f'let url = "{url}";',
    ]
    builder = f"session.http.{method}(url)"
    if query:
        pairs = ", ".join(f'("{k}", "{v}")' for k, v in query.items())
        lines.append(f"let query = [{pairs}];")
        builder += "\n    .query(&query)"
    for k, v in flow.request.headers.items():
        if k.lower() in ("content-type", "accept", "referer", "origin",
                          "x-requested-with", "x-csrf-token", "x-xsrf-token"):
            builder += f'\n    .header("{k}", "{v}")'
    if req_body_text:
        if is_json_req:
            lines.append("// cuerpo JSON observado (ver el .json para el detalle):")
            builder += "\n    .body(/* serde_json::json!({ ... }) */ String::new())"
        else:
            builder += f'\n    .body(r#"{req_body_text[:500]}"#)'
    lines.append(f"let resp = {builder}.send()?;")
    lines.append("let raw: serde_json::Value = resp.json()?;")
    lines.append("eprintln!(\"{}\", serde_json::to_string_pretty(&raw)?); // inspecciona y mapea")
    return "\n".join(lines)


def response(flow: http.HTTPFlow):
    global _counter
    if not _is_target(flow):
        return

    _counter += 1
    n = f"{_counter:03d}"

    req_body_text, req_is_json = _pretty(flow.request.content or b"")
    resp_body_text, resp_is_json = _pretty(flow.response.content or b"")
    interesting = _looks_interesting(resp_body_text, resp_is_json)

    record = {
        "n": _counter,
        "interesting": interesting,
        "method": flow.request.method,
        "url": flow.request.url,
        "status": flow.response.status_code,
        "request": {
            "headers": _filter_headers(flow.request.headers, KEEP_REQ_HEADERS),
            "query": dict(flow.request.query),
            "body": req_body_text,
        },
        "response": {
            "content_type": flow.response.headers.get("content-type", ""),
            "body": resp_body_text,
        },
    }

    stem = f"{n}_{flow.request.method}_{_slug(flow.request.path)}"
    (OUT_DIR / f"{stem}.json").write_text(
        json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # Índice incremental.
    with (OUT_DIR / "index.jsonl").open("a", encoding="utf-8") as idx:
        idx.write(json.dumps({
            "n": _counter, "interesting": interesting,
            "method": flow.request.method, "status": flow.response.status_code,
            "url": flow.request.url.split("?")[0],
        }, ensure_ascii=False) + "\n")

    if interesting:
        (OUT_DIR / f"{stem}.reqwest.rs").write_text(
            _rust_snippet(flow, req_body_text, req_is_json), encoding="utf-8"
        )
        ctx.log.alert(
            f"[renfe] ★ {n} POSIBLE ENDPOINT DE VENTA  "
            f"{flow.request.method} {flow.request.path.split('?')[0]}  "
            f"→ {stem}.json + .reqwest.rs"
        )
    else:
        ctx.log.info(f"[renfe]   {n} {flow.request.method} "
                     f"{flow.request.path.split('?')[0]}  ({flow.response.status_code})")


def done():
    if _counter:
        ctx.log.info(
            f"[renfe] {_counter} flujos capturados en ./{OUT_DIR}/. "
            f"Revisa index.jsonl y los marcados con ★."
        )
