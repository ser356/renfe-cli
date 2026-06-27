"""
open_checkout.py — abre el navegador real (visible) ya logueado en la
pantalla de pago que arma `renfe buy`.

Qué hace
--------
`renfe buy` deja la compra armada en venta.renfe.com y exporta la sesión a un
`cookies.txt` (formato Netscape), pero pegar la `checkout_url` en un navegador
normal no sirve: esa sesión vive en las cookies del CLI, no en las del
navegador, y Renfe responde "ha pasado demasiado tiempo" (U014).

Este script automatiza solo el tramo de "metersela las cookies al navegador",
no el pago: abre Chrome/Edge real (sin --headless, visible), inyecta las
cookies vía WebDriver (que sí puede fijar cookies HttpOnly, a diferencia de
JS de la propia página) y navega a la `checkout_url`. A partir de ahí el
navegador queda abierto y el pago (Redsys + 3D Secure, captcha) lo completa
la persona a mano, igual que si hubiera llegado ahí navegando ella misma.

Dependencias
------------
Solo hace falta tener `python3`. El propio script crea un entorno virtual
aislado en `~/.renfe/open-checkout-venv` la primera vez que lo necesita,
instala `selenium` dentro y se relanza a sí mismo con ese intérprete — así
evita el bloqueo "externally-managed-environment" (PEP 668) que tienen los
Python de Homebrew/sistema y no toca nada fuera de `~/.renfe/`. Las
siguientes compras reutilizan el mismo entorno, no se recrea cada vez. El
driver del navegador (msedgedriver/chromedriver) NO hay que instalarlo a
mano: desde selenium 4.6, "Selenium Manager" lo descarga solo, a juego con la
versión de Chrome/Edge que tengas.

Uso
---
    # opción A: encadenado con `renfe buy --json`
    renfe buy -o salamanca -d peñaranda --date 2026-06-26 --train 4 --yes --json \
      | python3 tools/open_checkout.py -

    # opción B: a partir de los ficheros ya generados
    python3 tools/open_checkout.py --cookies renfe-buy-_YoOP.cookies.txt \
      --url 'https://venta.renfe.com/vol/formasDePagoEnlaces.do?c=_YoOP'

Aviso
-----
El cookies.txt y el JSON de `renfe buy` contienen sesión sensible. No los
subas a ningún sitio; bórralos cuando termines de pagar.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

# Entorno virtual dedicado y persistente: evitamos pip global (bloqueado por
# PEP 668 en Python de Homebrew/sistema) y evitamos recrearlo en cada compra.
RENFE_HOME = Path(os.environ.get("RENFE_HOME", Path.home() / ".renfe"))
VENV_DIR = RENFE_HOME / "open-checkout-venv"
_RELAUNCH_MARKER = "_RENFE_OPEN_CHECKOUT_VENV"


def _venv_python() -> Path:
    if sys.platform == "win32":
        return VENV_DIR / "Scripts" / "python.exe"
    return VENV_DIR / "bin" / "python3"


def ensure_venv_and_relaunch() -> None:
    """Si no estamos ya corriendo dentro de `VENV_DIR`, lo crea si falta,
    instala `selenium` ahí dentro si falta, y vuelve a lanzar este mismo
    script con el intérprete del venv (sustituyendo el proceso actual)."""
    if os.environ.get(_RELAUNCH_MARKER) == "1":
        return  # ya estamos dentro del venv; seguir con normalidad

    vpy = _venv_python()
    if not vpy.exists():
        print(f"Creando entorno aislado en {VENV_DIR} (una sola vez)...", file=sys.stderr)
        RENFE_HOME.mkdir(parents=True, exist_ok=True)
        result = subprocess.run([sys.executable, "-m", "venv", str(VENV_DIR)])
        if result.returncode != 0 or not vpy.exists():
            raise SystemExit(
                f"no se pudo crear el entorno virtual en {VENV_DIR}. "
                "Instala python3-venv o créalo a mano con: "
                f"{sys.executable} -m venv {VENV_DIR}"
            )

    check = subprocess.run([str(vpy), "-c", "import selenium"], capture_output=True)
    if check.returncode != 0:
        print("Instalando selenium en el entorno aislado...", file=sys.stderr)
        install = subprocess.run([str(vpy), "-m", "pip", "install", "-q", "selenium"])
        if install.returncode != 0:
            raise SystemExit(
                f"no se pudo instalar selenium en {VENV_DIR}. Instálalo a mano con: "
                f"{vpy} -m pip install selenium"
            )

    os.environ[_RELAUNCH_MARKER] = "1"
    os.execv(str(vpy), [str(vpy), __file__, *sys.argv[1:]])


def parse_netscape_cookies(path: Path) -> list[dict]:
    """Lee un cookies.txt formato Netscape (el que escribe `renfe buy`)."""
    cookies = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 7:
            continue
        _domain, _include_subdomains, path_, secure, expires, name, value = parts
        # Sin "domain": WebDriver lo asigna al host de la página actual. Pasar
        # el dominio explícito hace que algunos drivers (msedgedriver) lo
        # rechacen con "invalid cookie domain" aunque coincida exactamente.
        cookie = {
            "name": name,
            "value": value,
            "path": path_,
            "secure": secure.upper() == "TRUE",
        }
        if expires not in ("0", ""):
            cookie["expiry"] = int(expires)
        cookies.append(cookie)
    if not cookies:
        raise SystemExit(f"{path}: no se ha podido parsear ninguna cookie")
    return cookies


def load_buy_output(source: str) -> dict:
    """Acepta la salida JSON de `renfe buy --json`: fichero o '-' (stdin)."""
    raw = sys.stdin.read() if source == "-" else Path(source).read_text(encoding="utf-8")
    data = json.loads(raw)
    for key in ("checkout_url", "cookies_file"):
        if key not in data:
            raise SystemExit(f"el JSON no trae «{key}»: ¿es la salida de `renfe buy --json`?")
    return data


# Debe coincidir EXACTAMENTE con `USER_AGENT` en src/api/mod.rs: la sesión la
# creó ese cliente, y algunos WAF (F5/Akamai) atan las cookies de sesión al
# User-Agent que las pidió. Si el navegador real navega con su propio UA, el
# WAF puede verlo como un secuestro de sesión y devolver "ha pasado demasiado
# tiempo" (U014) aunque las cookies sean correctas.
RENFE_USER_AGENT = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0 Safari/537.36"
)


def make_driver(browser: str, detach: bool = True):
    """Levanta el navegador real (visible) que pidas, o Edge y si falla Chrome.

    Con `detach=True` el navegador sigue abierto aunque este proceso (y el
    servicio del driver) termine; sin esto, Chromium lo cierra en cuanto el
    driver se desconecta, justo cuando el script acaba de dejarlo listo.
    En modo auto-pay usamos detach=False para mantener el control hasta que
    la compra se confirme."""
    from selenium import webdriver
    from selenium.common.exceptions import WebDriverException
    from selenium.webdriver.chrome.options import Options as ChromeOptions
    from selenium.webdriver.edge.options import Options as EdgeOptions

    def edge():
        opts = EdgeOptions()
        if detach:
            opts.add_experimental_option("detach", True)
        opts.add_argument(f"--user-agent={RENFE_USER_AGENT}")
        return webdriver.Edge(options=opts)

    def chrome():
        opts = ChromeOptions()
        if detach:
            opts.add_experimental_option("detach", True)
        opts.add_argument(f"--user-agent={RENFE_USER_AGENT}")
        return webdriver.Chrome(options=opts)

    builders = {"edge": edge, "chrome": chrome}
    order = [browser] if browser != "auto" else ["edge", "chrome"]
    last_err: Exception | None = None
    for name in order:
        try:
            return builders[name]()
        except WebDriverException as e:
            last_err = e
    raise SystemExit(
        "no se pudo arrancar ni Edge ni Chrome. ¿Tienes alguno de los dos "
        "instalado? (el driver lo descarga solo Selenium Manager, no hace "
        f"falta instalarlo a mano). Último error: {last_err}"
    )


def open_checkout(cookies: list[dict], checkout_url: str, browser: str, bizum: bool = True) -> None:
    driver = make_driver(browser, detach=not bizum)
    try:
        # Primer aterrizaje en el dominio para poder fijar las cookies.
        # Usamos la home, NO el checkout_url, para evitar exponer el ?c=<id>
        # a Renfe sin una sesión válida — hacerlo invalida el carrito (RV51).
        home = "https://venta.renfe.com/vol/homeCustomers.do"
        driver.get(home)
        for cookie in cookies:
            try:
                driver.add_cookie(cookie)
            except Exception:
                pass  # cookies con atributos no soportados; ignorar
        driver.get(checkout_url)
    except Exception:
        driver.quit()
        raise

    if bizum:
        try:
            bizum_flow(driver)
        except Exception:
            driver.quit()
            raise
        return

    print(f"Navegador abierto en {checkout_url}. Completa el pago ahí; este script no lo toca.")
    # No se llama a driver.quit(): queremos que el navegador siga abierto.


def _fill_buyer_fields(driver, wait_for) -> None:
    """Rellena email y teléfono del comprador vía JS y descarta el banner de cookies."""
    from selenium.webdriver.common.by import By
    from selenium.webdriver.support import expected_conditions as EC

    # Descartar banner OneTrust si aparece.
    try:
        wait_for(driver, 6).until(
            EC.element_to_be_clickable((By.ID, "onetrust-accept-btn-handler"))
        ).click()
        import time; time.sleep(0.5)
    except Exception:
        pass

    def set_field(fid: str, value: str) -> None:
        driver.execute_script(
            "var el=document.getElementById(arguments[0]);"
            "if(el){el.value=arguments[1];"
            "el.dispatchEvent(new Event('input',{bubbles:true}));"
            "el.dispatchEvent(new Event('change',{bubbles:true}));}",
            fid, value,
        )

    buyer_email = os.environ.get("RENFE_BUYER_EMAIL", "")
    buyer_phone = os.environ.get("RENFE_BUYER_PHONE", "")
    if buyer_email:
        set_field("inputEmail", buyer_email)
    if buyer_phone:
        set_field("telefonoComprador", buyer_phone)

    # Marcar "He leído y acepto las condiciones" — sin esto butonPagar
    # permanece disabled independientemente del método de pago elegido.
    # Usamos click() en lugar de .checked=true porque Renfe escucha 'click',
    # no el evento 'change'.
    driver.execute_script(
        "var cb=document.getElementById('aceptarCondiciones');"
        "if(cb&&!cb.checked){cb.scrollIntoView();cb.click();}"
    )


def bizum_flow(driver) -> None:
    """Selecciona Bizum, rellena datos del comprador y envía el formulario.

    Flujo:
      1. formasDePagoEnlaces.do → seleccionar radio Bizum + rellenar
         email/teléfono + click butonPagar
      2. El navegador aterriza en la página de confirmación de Bizum.
         El usuario aprueba desde su móvil; el script solo espera
         el retorno a venta.renfe.com.
    """
    import time
    from selenium.webdriver.common.by import By
    from selenium.webdriver.support.ui import WebDriverWait
    from selenium.webdriver.support import expected_conditions as EC

    def wait_for(driver, timeout=30):
        return WebDriverWait(driver, timeout)

    # ── 1. Preparar el formulario de formasDePagoEnlaces.do ─────────────────
    print("  [1/2] Seleccionando Bizum...", file=sys.stderr, flush=True)
    try:
        # Esperar a que el formulario cargue.
        wait_for(driver).until(
            EC.presence_of_element_located((By.ID, "formBean"))
        )

        # Rellenar datos del comprador y descartar banner.
        _fill_buyer_fields(driver, wait_for)
        time.sleep(0.3)

        # Seleccionar radio Bizum vía JS.
        driver.execute_script(
            "var r=document.getElementById('datosPago_cdgoFormaPago_bizum');"
            "if(r&&!r.checked){r.click();"
            "r.dispatchEvent(new Event('change',{bubbles:true}));}"
        )
        time.sleep(0.5)

        # Esperar a que butonPagar se habilite y hacer clic.
        wait_for(driver, timeout=15).until(
            lambda d: not d.find_element(By.ID, "butonPagar").get_attribute("disabled")
        )
        driver.execute_script("document.getElementById('butonPagar').click();")
    except Exception as e:
        raise SystemExit(
            f"No se pudo seleccionar Bizum en la página de pago.\n"
            f"¿La sesión sigue activa? Error: {e}"
        )

    # ── 2. Esperar a que el usuario apruebe en su móvil ──────────────────────
    print(
        "  [2/2] Formulario enviado. Aprueba el pago en tu app de Bizum.",
        file=sys.stderr, flush=True,
    )
    # Esperamos que la URL vuelva a venta.renfe.com con la confirmación.
    # El usuario tiene hasta 5 minutos para aprobar.
    try:
        WebDriverWait(driver, 300).until(
            lambda d: (
                "venta.renfe.com" in d.current_url
                and any(k in d.current_url for k in ("respuestaRedSys", "confirmacion", "ok"))
            )
        )
        print("\nCompra confirmada. Revisa tu email.", file=sys.stderr, flush=True)
    except Exception:
        print(
            "\nTiempo de espera agotado. Comprueba el navegador y tu app de Bizum.",
            file=sys.stderr, flush=True,
        )
    # Dejar el navegador abierto para que el usuario vea la confirmación.


def main() -> None:
    ensure_venv_and_relaunch()

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    ap.add_argument(
        "buy_json", nargs="?",
        help="Fichero con la salida de `renfe buy --json`, o '-' para leerla de stdin",
    )
    ap.add_argument("--cookies", help="cookies.txt de `renfe buy` (si no usas buy_json)")
    ap.add_argument("--url", help="checkout_url de `renfe buy` (si no usas buy_json)")
    ap.add_argument("--browser", choices=["auto", "edge", "chrome"], default="auto")
    ap.add_argument(
        "--no-bizum", dest="bizum", action="store_false",
        help="No seleccionar Bizum automáticamente; dejar el navegador abierto "
             "para completar el pago a mano.",
    )
    ap.set_defaults(bizum=True)
    args = ap.parse_args()

    if args.buy_json:
        data = load_buy_output(args.buy_json)
        cookies_path = Path(data["cookies_file"])
        checkout_url = data["checkout_url"]
    elif args.cookies and args.url:
        cookies_path = Path(args.cookies)
        checkout_url = args.url
    else:
        ap.error("pasa el JSON de `renfe buy --json` o bien --cookies y --url")

    cookies = parse_netscape_cookies(cookies_path)
    open_checkout(cookies, checkout_url, args.browser, bizum=args.bizum)


if __name__ == "__main__":
    main()
