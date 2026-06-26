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
| Búsqueda / precio / plaza (`search`) | **pendiente de captura** | `venta.renfe.com` (privado) |
| Tarifas y selección de plaza | pendiente de captura | `venta.renfe.com` |
| Pago (Redsys + 3DS) | **fuera de alcance por diseño** | — |

La parte de venta exige capturar el tráfico real del navegador (ver
[`CAPTURA.md`](CAPTURA.md)). El código deja las firmas listas y marcadas; no se
incluyen endpoints inventados.

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

# Búsqueda (tras implementar sale::search)
renfe search -o madrid -d barcelona --date 2026-07-01 --sort precio
renfe search -o 60000 -d 71801 --available-only --json

# Telemetría en tiempo real (sin login)
renfe track            # toda la flota de largo recorrido activa
renfe track AVE        # filtrar por servicio o número de tren
renfe track --json

# Perfiles (para la sesión autenticada de venta)
renfe profile set yo --token "<cookie-de-sesión>" --email tu@email.es
renfe profile use yo
renfe whoami
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
    sale.rs          flujo de venta multi-paso             [pendiente captura]
  commands/          un módulo por subcomando
```

## Diseño: por qué el pago queda fuera

El flujo de venta de Renfe termina en pasarela de pago (Redsys) con 3D Secure,
que dispara autenticación del banco emisor fuera del proceso. Automatizarlo es
frágil, viola los términos de servicio con riesgo de bloqueo de cuenta, y acerca
el proyecto a obligaciones PCI-DSS al manipular datos de tarjeta. Por eso el CLI
llega hasta dejar la compra armada y delega el pago al navegador.

## Licencia

ISC.
