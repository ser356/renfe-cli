# renfe-cli

CLI no oficial en Rust para la API privada de `venta.renfe.com`: búsqueda de
trenes con precio y disponibilidad de plaza, telemetría en tiempo real y salida
`--json` para automatización.

> No afiliado ni respaldado por Renfe. La API es privada, sin documentar y sin
> contrato de uso: puede cambiar o dejar de funcionar sin previo aviso. Úsalo
> solo con cuentas y datos que estés autorizado a usar y respeta los límites de
> petición. "Renfe" es marca de Renfe-Operadora; se menciona solo para describir
> compatibilidad.

## Estado

| Área | Estado | Fuente |
|------|--------|--------|
| Catálogo de estaciones | **funcional** | `estacionesEstaticas.js` (público) |
| Telemetría de flota (`track`) | **funcional** | `flotaLD.json` (sin auth) |
| Perfiles locales | **funcional** | `~/.renfe/` |
| Búsqueda / precio / plaza (`search`) | **funcional** | `venta.renfe.com` (DWR, ★ de `capturas/`) |
| Tarifas por tren | **funcional** | vienen embebidas en `search()`, no es un paso aparte |
| Armado de carrito (`buy`) | **funcional hasta antes del pago** | `venta.renfe.com` (multi-paso con estado de sesión) |
| Pago (Redsys + 3DS) | **fuera de alcance por diseño** | — |

La parte de venta se implementó a partir de tráfico real capturado del
navegador (ver [`CAPTURA.md`](CAPTURA.md) y `capturas/`); no hay endpoints
inventados. Si Renfe cambia el esquema de respuesta, el parseo defensivo
(`sale.rs`) debería degradar con un error claro en vez de un panic — si no,
hace falta una nueva captura.

## Build

Requiere Rust estable reciente. Con toolchains antiguas (1.75) el `Cargo.lock`
incluido ya fija versiones compatibles.

```
cargo build --release
./target/release/renfe --help
```

## Uso

```
# Estaciones
renfe stations atocha
renfe stations --json
renfe stations --refresh        # ignora la caché local y recarga el catálogo

# Búsqueda: horarios, precio mínimo y disponibilidad
renfe search -o madrid -d barcelona --date 2026-07-01 --sort precio
renfe search -o 60000 -d 71801 --available-only --json
renfe search -o madrid -d barcelona --date 2026-07-01 --return-date 2026-07-05

# Telemetría en tiempo real (sin login)
renfe track            # toda la flota de largo recorrido activa
renfe track AVE        # filtrar por servicio o número de tren
renfe track --json

# Perfiles: token de sesión + datos del viajero (necesarios para `buy`)
renfe profile set yo \
  --token "<cookie-de-sesión, ver CAPTURA.md>" \
  --email tu@email.es \
  --nombre Ada --apellido1 Lovelace --apellido2 "" \
  --tipo-documento dni --documento 12345678A \
  --prefijo +34 --telefono 600000000
renfe profile use yo
renfe profile list
renfe whoami

# Armar la compra (1 adulto, solo ida) hasta justo antes del pago
renfe search -o madrid -d barcelona --date 2026-07-01   # anota el id de "Tren"
renfe buy -o madrid -d barcelona --date 2026-07-01 --train 1
renfe buy -o madrid -d barcelona --date 2026-07-01 --train 1 --fare VR010 --yes --open
```

`renfe buy` deja el carrito armado y devuelve una `checkout_url` más un
`cookies.txt` (formato Netscape) en disco. El pago en sí (Redsys + 3D Secure)
se hace en el navegador, importando esas cookies si la sesión no se ata sola;
el CLI nunca lo automatiza (ver más abajo). El fichero de cookies contiene
sesión sensible: bórralo después de pagar.

Pegar la `checkout_url` directamente en un navegador normal **no funciona**:
esa sesión vive en las cookies que recogió el CLI, no en las del navegador, y
Renfe responde "ha pasado demasiado tiempo" (U014). Con `--open`, `renfe buy`
abre por ti un navegador real (Chrome o Edge, visible, nunca headless), le
inyecta la sesión vía WebDriver y te deja ya en la pantalla de pago — el pago
en sí (Redsys + 3D Secure, captcha) lo sigue completando la persona a mano,
el CLI no lo toca:

```
renfe buy -o salamanca -d "peñaranda de bracamonte" --date 2026-06-26 --train 4 --yes --open
```

Solo necesitas tener `python3` instalado. La primera vez, el script crea un
entorno virtual propio en `~/.renfe/open-checkout-venv` e instala `selenium`
ahí dentro (evita el bloqueo `externally-managed-environment` de los Python
de Homebrew/sistema en macOS); las compras siguientes reutilizan ese mismo
entorno. El driver del navegador (msedgedriver/chromedriver) tampoco hay que
instalarlo a mano — desde selenium 4.6, "Selenium Manager" lo descarga solo a
juego con tu versión de Chrome/Edge.

El binario lleva embebido el script que hace la inyección (no depende de
tener el repo a mano), pero el original está en `tools/open_checkout.py` por
si prefieres usarlo suelto contra un `cookies.txt` ya generado:

```
python3 tools/open_checkout.py --cookies renfe-buy-_YoOP.cookies.txt \
  --url 'https://venta.renfe.com/vol/formasDePagoEnlaces.do?c=_YoOP'
```

`--json` y `-p <perfil>` son flags globales.

## Arquitectura

```
src/
  main.rs            entry + dispatch
  cli.rs             definición de comandos (clap derive)
  models.rs          structs de dominio
  config/mod.rs      perfiles en ~/.renfe/profiles.json
  api/
    session.rs       cliente reqwest con cookie_store (estado de venta)
    stations.rs      catálogo + caché + resolución fuzzy  [funcional]
    telemetry.rs     flotaLD.json                          [funcional]
    sale.rs          búsqueda (DWR) + armado de carrito    [funcional, sin pago]
  commands/
    stations.rs / track.rs / search.rs / profile.rs / buy.rs
tools/
  capture_renfe.py   addon mitmproxy para capturar nuevos flujos de venta
```

## Diseño: por qué el pago queda fuera

El flujo de venta de Renfe termina en pasarela de pago (Redsys) con 3D Secure,
que dispara autenticación del banco emisor fuera del proceso. Automatizarlo es
frágil, viola los términos de servicio con riesgo de bloqueo de cuenta, y acerca
el proyecto a obligaciones PCI-DSS al manipular datos de tarjeta. Por eso el CLI
llega hasta dejar la compra armada y delega el pago al navegador.

## Licencia

ISC.
