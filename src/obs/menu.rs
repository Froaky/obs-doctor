//! Names exactly as OBS 33 shows them with the interface in Spanish
//! (taken from OBS's own `es-ES.ini` locale files). Advice quotes these so
//! the streamer finds each option without translating in their head.

pub const SETTINGS: &str = "Ajustes";
pub const OUTPUT: &str = "Salida";
pub const VIDEO: &str = "Vídeo";
pub const STREAM: &str = "Emisión";
pub const OUTPUT_MODE: &str = "Modo de salida";
pub const MODE_SIMPLE: &str = "Sencillo";
pub const MODE_ADVANCED: &str = "Avanzado";
pub const ENCODER: &str = "Codificador de vídeo";
pub const BITRATE: &str = "Bitrate de vídeo";
pub const OUTPUT_RES: &str = "Resolución de salida (Escalada)";
pub const FPS: &str = "Valores comunes de FPS";
pub const RATE_CONTROL: &str = "Control de la frecuencia";
/// Simple mode calls it one way, advanced x264 the other.
pub const PRESET_SIMPLE: &str = "Preajuste del codificador";
pub const PRESET_X264: &str = "Perfil de uso de CPU";
pub const AUTO_CONFIG: &str = "Asistente de configuración automática";
pub const ADVANCED: &str = "Avanzado";
/// Group and checkbox in Ajustes > Avanzado.
pub const RECONNECT: &str = "Reconectar automáticamente";

/// "En OBS abre Ajustes > Salida."
pub fn open(tab: &str) -> String {
    format!("En OBS abre {SETTINGS} > {tab}.")
}
