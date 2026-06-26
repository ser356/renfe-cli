use clap::{Args, Parser, Subcommand};

/// CLI no oficial para venta.renfe.com.
///
/// No afiliado ni respaldado por Renfe. La API es privada y puede cambiar o
/// dejar de funcionar sin previo aviso. Úsalo solo con cuentas y datos que
/// estés autorizado a usar, y respeta los límites de petición.
#[derive(Parser, Debug)]
#[command(name = "renfe", version, about, long_about = None)]
pub struct Cli {
    /// Salida en JSON para automatización (suprime tablas y mensajes humanos).
    #[arg(long, global = true)]
    pub json: bool,

    /// Perfil a usar (por defecto el activo en ~/.renfe/).
    #[arg(short = 'p', long, global = true)]
    pub profile: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Catálogo de estaciones (búsqueda por nombre o código).
    Stations(StationsArgs),
    /// Buscar trenes: horarios, precio y disponibilidad de plaza.
    Search(SearchArgs),
    /// Telemetría en tiempo real de un tren en circulación (sin login).
    Track(TrackArgs),
    /// Gestión de perfiles locales (token, viajero por defecto).
    Profile(ProfileArgs),
    /// Mostrar el perfil activo.
    Whoami,
    /// Armar carrito de compra (solo ida, 1 adulto). El pago se delega al
    /// navegador: el comando deja la sesión lista en formasDePagoEnlaces.do
    /// y exporta cookies para que abras la URL final tú mismo.
    Buy(BuyArgs),
}

#[derive(Args, Debug)]
pub struct StationsArgs {
    /// Texto a buscar (nombre de estación o ciudad). Vacío = listar todas.
    pub query: Option<String>,
    /// Forzar recarga del catálogo desde renfe.com (ignora caché local).
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Origen: código de estación o texto (se resuelve contra el catálogo).
    #[arg(short = 'o', long)]
    pub origin: String,
    /// Destino: código de estación o texto.
    #[arg(short = 'd', long)]
    pub destination: String,
    /// Fecha de ida (YYYY-MM-DD). Por defecto, hoy.
    #[arg(long)]
    pub date: Option<String>,
    /// Fecha de vuelta (YYYY-MM-DD). Opcional.
    #[arg(long)]
    pub return_date: Option<String>,
    /// Número de adultos.
    #[arg(long, default_value_t = 1)]
    pub adults: u8,
    /// Ordenar por: salida | precio | duracion.
    #[arg(long, default_value = "salida")]
    pub sort: String,
    /// Mostrar solo trenes con plaza disponible.
    #[arg(long)]
    pub available_only: bool,
}

#[derive(Args, Debug)]
pub struct TrackArgs {
    /// Número de tren (cdgoTren) o filtro libre. Vacío = toda la flota activa.
    pub train: Option<String>,
}

#[derive(Args, Debug)]
pub struct BuyArgs {
    /// Origen: código de estación o texto.
    #[arg(short = 'o', long)]
    pub origin: String,
    /// Destino: código de estación o texto.
    #[arg(short = 'd', long)]
    pub destination: String,
    /// Fecha de ida (YYYY-MM-DD).
    #[arg(long)]
    pub date: String,
    /// `id` del tren a comprar (columna "Tren" en `renfe search`).
    #[arg(long)]
    pub train: i64,
    /// Código de tarifa exacto (p. ej. VR010). Si se omite, la primera disponible.
    #[arg(long)]
    pub fare: Option<String>,
    /// Ruta donde escribir el cookies.txt (formato Netscape) para curl/wget/Firefox.
    /// Por defecto `./renfe-buy-<idCompra>.cookies.txt`.
    #[arg(long)]
    pub cookies_out: Option<String>,
    /// Saltar la confirmación interactiva.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Tras armar el carrito, abre un navegador real (Chrome/Edge, visible)
    /// con la sesión ya inyectada, en la pantalla de pago. Requiere
    /// `python3`; instala `selenium` solo si falta. El pago en sí lo
    /// completa la persona; esto solo evita pegar la cookie a mano.
    #[arg(long)]
    pub open: bool,
}

#[derive(Args, Debug)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub action: ProfileAction,
}

#[derive(Subcommand, Debug)]
pub enum ProfileAction {
    /// Listar perfiles guardados.
    List,
    /// Crear o actualizar un perfil. Solo se cambian los campos indicados.
    Set {
        name: String,
        /// JWT/cookie de sesión capturado del navegador (ver CAPTURA.md).
        #[arg(long)]
        token: Option<String>,
        /// Email de la cuenta.
        #[arg(long)]
        email: Option<String>,
        /// Nombre del titular (para `renfe buy`).
        #[arg(long)]
        nombre: Option<String>,
        /// Primer apellido del titular.
        #[arg(long)]
        apellido1: Option<String>,
        /// Segundo apellido del titular.
        #[arg(long)]
        apellido2: Option<String>,
        /// Tipo de documento: dni | nie | pasaporte (default: dni).
        #[arg(long, value_parser = ["dni", "nie", "pasaporte"])]
        tipo_documento: Option<String>,
        /// Número de documento.
        #[arg(long)]
        documento: Option<String>,
        /// Prefijo telefónico (default "+34").
        #[arg(long)]
        prefijo: Option<String>,
        /// Teléfono móvil sin prefijo.
        #[arg(long)]
        telefono: Option<String>,
    },
    /// Marcar un perfil como activo.
    Use { name: String },
}
