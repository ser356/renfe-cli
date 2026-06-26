# CLAUDE.md — renfe-cli

Contexto operativo para trabajar en este repo. Léelo antes de tocar código.

## Qué es

CLI no oficial en Rust sobre la API privada de `venta.renfe.com`: búsqueda de
trenes con precio y disponibilidad, telemetría en tiempo real y salida `--json`
para automatización. Sigue el patrón de un wrapper de API privada con perfiles
locales (estilo `mercadona-cli`), adaptado a que Renfe no documenta su venta.

Binario: `renfe`. Estado local: `~/.renfe/`.

## Reglas de diseño (NO negociables)

1. **El pago queda fuera de alcance.** No implementes checkout de pago: el flujo
   termina en Redsys + 3D Secure, que no se automatiza (fragilidad, términos de
   servicio, PCI-DSS). El CLI llega hasta dejar la compra armada y delega el
   pago al navegador. No cambies esto sin que el usuario lo pida explícitamente.
2. **No inventes endpoints.** La parte de venta (`src/api/sale.rs`) solo se
   rellena con tráfico real capturado por el usuario (ver `CAPTURA.md` y
   `tools/capture_renfe.py`). Si no hay captura disponible para algo, deja un
   `bail!` con un TODO claro; nunca cablees una URL o un cuerpo supuestos.
3. **Parseo defensivo siempre.** El JSON de Renfe cambia sin avisar. Prueba
   varios nombres de clave candidatos (como en `telemetry.rs` y `stations.rs`),
   nunca asumas uno solo. Mapea sobre `serde_json::Value` y construye los structs
   de `models.rs` con tolerancia a campos ausentes.
4. **Salida dual.** Todo comando respeta `--json` (estructurado, sin adornos,
   para scripting) y, en su ausencia, una tabla legible (`comfy-table`). Los
   mensajes humanos van a `stderr`; los datos a `stdout`.
5. **Datos sensibles nunca al repo.** `~/.renfe/` y `./capturas/` contienen
   tokens y cookies en claro. Ya están en `.gitignore`. No los vuelques en logs
   ni en tests.
6. **Idioma y estilo.** Comentarios, mensajes de error y de usuario en español,
   registro impersonal y preciso. Sin emojis en la salida del CLI.

## Estado por módulo

| Módulo | Estado | Notas |
|--------|--------|-------|
| `api/stations.rs` | funcional | catálogo `estacionesEstaticas.js` + caché + resolución fuzzy. Verifica nombres de clave contra el JS real. |
| `api/telemetry.rs` | funcional | `flotaLD.json`, sin auth, solo largo recorrido. Ajusta nombres de campo si `track` viene vacío. |
| `api/session.rs` | funcional | cliente `reqwest` blocking con `cookie_store` compartido. `auth_header()` pendiente de confirmar nombre de cookie. |
| `config/mod.rs` | funcional | perfiles en `~/.renfe/profiles.json`. |
| `api/sale.rs` | **pendiente** | `search()` y `fares()` con `bail!`. Rellenar tras captura. |
| `commands/search.rs` | listo salvo datos | toda la tubería (resolución, orden, filtro, salida) está hecha; solo espera que `sale::search` devuelva datos. |
| pago / checkout | fuera de alcance | por diseño. |

## Arquitectura

```
src/
  main.rs            entry + dispatch de subcomandos
  cli.rs             definición de comandos y flags (clap derive)
  models.rs          structs de dominio (Station, TrainOption, Fare, TrainPosition)
  config/mod.rs      perfiles ~/.renfe/
  api/
    mod.rs           USER_AGENT compartido
    session.rs       Session: cliente HTTP con estado (cookies entre pasos)
    stations.rs      catálogo + caché + resolve()          [funcional]
    telemetry.rs     flotaLD.json                          [funcional]
    sale.rs          flujo de venta multi-paso             [pendiente captura]
  commands/
    mod.rs           helper table()
    stations.rs / track.rs / search.rs / profile.rs
tools/
  capture_renfe.py   addon mitmproxy: captura y exporta el flujo de venta
```

## Tarea principal

Implementar la búsqueda de venta una vez el usuario aporte capturas en
`./capturas/` (generadas por `tools/capture_renfe.py`; los flujos marcados con ★
traen un `.reqwest.rs` de referencia).

Pasos:

1. Abre `capturas/index.jsonl`, localiza el flujo ★ del POST de búsqueda.
2. En `sale::search()`, sustituye el `bail!` por la petición real con
   `session.http` (reutiliza la cookie store; inyecta auth vía
   `Session::auth_header()`, no a mano).
3. Vuelca primero la respuesta cruda con `eprintln!` para ver la forma, luego
   escribe un `parse_search(raw) -> Vec<TrainOption>` defensivo.
4. Repite para `fares()` (paso 2 del flujo: tarifas/clases de un tren).
5. Quita los warnings de dead-code (`return_date`, `auth_header`) al conectarlos.

El flujo de venta de Renfe es multi-paso y con estado de sesión: búsqueda →
selección de tren → tarifas → bloqueo de plaza → datos de viajero. Cada paso
depende de cookies del anterior. No fusiones pasos.

## Convenciones de código

- Errores con `anyhow::Result` + `.context(...)` describiendo la operación.
- `reqwest` en modo blocking; nada de async salvo que haya razón fuerte.
- Tablas con el helper `commands::table(&[...])`.
- No añadas dependencias sin necesidad real. Mantén compatibilidad con
  toolchains antiguas (el `Cargo.lock` está pineado; en toolchain reciente puede
  regenerarse libremente).
- Tests: parseo puro (catálogo, telemetría, respuestas de venta) con fixtures
  JSON locales en `tests/`; nunca hagas red en tests.

## Comandos

```
cargo build --release        # binario en target/release/renfe
cargo run -- <args>          # ejecutar en debug
cargo check                  # comprobación rápida
cargo clippy                 # lints (resuelve warnings antes de cerrar)
cargo fmt                    # formato
```

## Definición de "hecho" para `search`

- `renfe search -o madrid -d barcelona --date <futuro>` devuelve trenes reales
  con precio y disponibilidad, tanto en tabla como con `--json`.
- `--sort precio|duracion|salida` y `--available-only` operan sobre datos reales.
- Parseo tolerante a campos ausentes; si Renfe responde vacío o cambia el
  esquema, el error es claro, no un panic.
- Sin warnings de `cargo clippy`. Datos sensibles fuera de logs.
