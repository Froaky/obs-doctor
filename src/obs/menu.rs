//! Names exactly as OBS 33 shows them with the interface in Spanish
//! (taken from OBS's own `es-ES.ini` locale files). Advice quotes these so
//! the streamer finds each option without translating in their head.

pub const SETTINGS: &str = "Ajustes";
pub const OUTPUT: &str = "Salida";
pub const VIDEO: &str = "Vídeo";
pub const STREAM: &str = "Emisión";
pub const ENCODER: &str = "Codificador de vídeo";
pub const BITRATE: &str = "Bitrate de vídeo";
pub const OUTPUT_RES: &str = "Resolución de salida (Escalada)";
pub const FPS: &str = "Valores comunes de FPS";
pub const RATE_CONTROL: &str = "Control de la frecuencia";
/// Simple mode calls it one way, advanced x264 the other.
pub const PRESET_SIMPLE: &str = "Preajuste del codificador";
pub const PRESET_X264: &str = "Perfil de uso de CPU";
pub const ADVANCED: &str = "Avanzado";
/// Group and checkbox in Ajustes > Avanzado.
pub const RECONNECT: &str = "Reconectar automáticamente";

// Advanced output mode, one label per encoder plugin (each plugin's own
// es-ES.ini). The x264 plugin abbreviates; the NVENC one does not.
/// x264 and QSV: "Intervalo de fotogramas clave (0=auto)".
pub const KEYFRAME: &str = "Intervalo de fotogramas clave";
/// NVENC and AMF.
pub const ENC_PRESET: &str = "Preajuste";
pub const MULTIPASS: &str = "Modo multipaso";
pub const MULTIPASS_QRES: &str = "Dos pasos (resolución de un cuarto)";
pub const LOOKAHEAD: &str = "Mirar hacia adelante";
/// QuickSync.
pub const TARGET_USAGE: &str = "Uso de destino";
/// Ajustes > Salida, advanced mode, stream tab.
pub const RESCALE: &str = "Cambiar la escala de salida";
/// Ajustes > Vídeo.
pub const SCALE_FILTER: &str = "Filtro de escala";
pub const SCALE_BICUBIC: &str = "Bicúbico";
pub const AUDIO_BITRATE: &str = "Bitrate de audio";
/// Simple mode, recording block.
pub const REC_QUALITY: &str = "Calidad de grabación";
pub const REC_SAME_AS_STREAM: &str = "Igual a la emisión";
/// Advanced mode, recording tab, first entry of its encoder list (OBS's
/// own spelling).
pub const REC_USE_STREAM_ENCODER: &str = "(Usar codificación de trasmisión)";
pub const RECORDING: &str = "Grabación";
/// Ajustes > Emisión.
pub const IGNORE_RECOMMENDED: &str = "Ignorar las recomendaciones de configuración del servicio de streaming";

/// "En OBS abre Ajustes > Salida."
pub fn open(tab: &str) -> String {
    format!("En OBS abre {SETTINGS} > {tab}.")
}
