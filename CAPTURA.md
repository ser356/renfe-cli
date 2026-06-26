# Guía de captura — flujo de venta de Renfe

El objetivo es observar las peticiones reales que hace `venta.renfe.com` para
rellenar `src/api/sale.rs`. No se asume nada del payload: todo sale de lo que
veas en la captura.

## 1. Método rápido: DevTools

1. Abre `https://www.renfe.com/es/es` en Chrome, DevTools → pestaña **Network**,
   filtro **Fetch/XHR**, marca **Preserve log**.
2. Haz una búsqueda real (origen, destino, fecha) y completa el flujo hasta el
   punto de pago (sin pagar).
3. Identifica, en orden, las peticiones de cada paso:
   - **Búsqueda**: la que devuelve la lista de trenes con horarios y precios
     "desde". Suele ser un POST a un endpoint bajo `venta.renfe.com`.
   - **Selección de tren**: al pinchar un tren, la petición que trae tarifas y
     clases.
   - **Bloqueo en carrito**: al elegir tarifa/plaza, la que reserva
     temporalmente.
4. Para cada una anota: **URL**, **método**, **headers** (sobre todo `Cookie`,
   `Content-Type`, cualquier token o `X-*`), y el **payload** (pestaña Payload).
5. En la respuesta, mira la estructura JSON (o HTML/DWR) y apunta los nombres de
   campo que mapean a `TrainOption` / `Fare` en `src/models.rs`.

## 2. Método robusto: mitmproxy

Para el flujo completo, incluida la app móvil (a veces usa una API más limpia
que la web):

```
mitmproxy --listen-port 8080
# Configura el dispositivo/navegador para usar el proxy e instala el cert de mitm.
# Filtra por el dominio:
#   ~d venta.renfe.com
```

La app oficial suele hablar con endpoints más estructurados que la web, que
arrastra arquitectura Java legacy (`HIRRenfeWeb`, acciones `.do`, posiblemente
DWR). Compara ambas y quédate con la más estable.

### Captura automatizada (recomendado)

En vez de bucear a mano, usa el addon incluido:

```
mitmweb -s tools/capture_renfe.py     # UI en http://127.0.0.1:8081
# o sin interfaz:  mitmdump -s tools/capture_renfe.py
```

Filtra solo los dominios de Renfe, ignora estáticos y analítica, y vuelca cada
flujo a `./capturas/NNN_<metodo>_<path>.json` con un `index.jsonl`. Marca **en
vivo** con ★ los que parecen traer trenes/precios y, para esos, genera un
`.reqwest.rs` de referencia listo para adaptar en `sale.rs`. Tras la captura,
abre `capturas/index.jsonl`, ve directo a los ★, y porta el snippet.

> `./capturas/` contiene tus cookies y token en claro. Trátala como `~/.renfe/`;
> ya está en `.gitignore`.

## 3. Rellenar el código

Con la captura delante:

- **`SALE_SEARCH_URL`** y el cuerpo del POST → `sale::search()`. Sustituye el
  `bail!` por la petición real usando `session.http` (mantiene cookies) y, si el
  endpoint requiere sesión, añade `session.auth_header()`.
- Escribe `parse_search(raw: serde_json::Value) -> Vec<TrainOption>` mapeando los
  campos observados. Mantén el estilo defensivo de `stations.rs`/`telemetry.rs`
  (probar varias claves) para sobrevivir a cambios menores.
- Para tarifas, repite en `sale::fares()`.

## 4. Token de sesión

El login de Renfe puede ir protegido (captcha, OTP). El patrón pragmático es el
mismo que usan otros CLIs sobre APIs privadas: **autenticarte en el navegador y
copiar la cookie de sesión** a un perfil:

```
renfe profile set yo --token "<valor de la cookie de sesión>"
```

Confirma en la captura **qué cookie** es la que autentica (nombre exacto) y, si
no es una simple `Cookie`, ajusta `Session::auth_header()` en
`src/api/session.rs`.

## 5. Higiene

- No pegues tokens ni datos personales reales en issues, commits ni
  documentación. Trata `~/.renfe/` como sensible (añádelo a tu `.gitignore`).
- Respeta rate limits: cachea el catálogo (ya se hace) y no hagas polling
  agresivo en `watch` (introduce backoff).
