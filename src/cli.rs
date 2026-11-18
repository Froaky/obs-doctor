//! Command-line options. Kept tiny on purpose: most people double-click.

use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    /// OBS config folder, for portable installs.
    pub obs_dir: Option<PathBuf>,
    /// Do not measure the internet connection.
    pub skip_network: bool,
    /// Print the whole report in the console instead of making the page.
    pub console_only: bool,
    /// Make the page but do not open the browser.
    pub no_open: bool,
    /// Where to save the page.
    pub output: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
}

pub const HELP: &str = "\
OBS Doctor: revisa tu PC, tu internet y tu OBS, y te dice qué cambiar para
transmitir mejor. No modifica nada: los cambios los haces tú.

USO
  obs-doctor [opciones]

OPCIONES
  --sin-internet       No usa internet (hoy tampoco lo usa sin esta opción)
  --consola            Muestra el reporte completo aquí, sin abrir el navegador
  --no-abrir           Guarda el reporte pero no lo abre
  --salida <archivo>   Dónde guardar el reporte (por defecto, %LOCALAPPDATA%\\obs-doctor\\reporte.html)
  --obs-dir <carpeta>  Carpeta de configuración de OBS (para instalaciones portables)
  -h, --ayuda          Muestra esta ayuda
  -V, --version        Muestra la versión
";

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" | "--ayuda" | "/?" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--sin-internet" | "--sin-red" => options.skip_network = true,
            "--consola" => options.console_only = true,
            "--no-abrir" => options.no_open = true,
            "--salida" => options.output = Some(path_value(&arg, args.next())?),
            "--obs-dir" => options.obs_dir = Some(path_value(&arg, args.next())?),
            other => {
                return Err(format!("No conozco la opción \"{other}\". Usa --ayuda para ver las opciones."));
            }
        }
    }
    Ok(Command::Run(options))
}

fn path_value(flag: &str, value: Option<String>) -> Result<PathBuf, String> {
    match value {
        Some(v) if !v.starts_with("--") && !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Err(format!("A la opción {flag} le falta su valor. Usa --ayuda para ver un ejemplo.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn no_arguments_runs_with_defaults() {
        assert_eq!(parse_str(&[]), Ok(Command::Run(Options::default())));
    }

    #[test]
    fn reads_every_option() {
        let Ok(Command::Run(options)) =
            parse_str(&["--sin-internet", "--consola", "--no-abrir", "--salida", "r.html", "--obs-dir", "D:\\OBS"])
        else {
            panic!("should parse");
        };
        assert!(options.skip_network && options.console_only && options.no_open);
        assert_eq!(options.output, Some(PathBuf::from("r.html")));
        assert_eq!(options.obs_dir, Some(PathBuf::from("D:\\OBS")));
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(parse_str(&["--consola", "--ayuda"]), Ok(Command::Help));
        assert_eq!(parse_str(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn unknown_options_and_missing_values_are_explained() {
        assert!(parse_str(&["--turbo"]).unwrap_err().contains("--turbo"));
        assert!(parse_str(&["--salida"]).unwrap_err().contains("--salida"));
        assert!(parse_str(&["--obs-dir", "--consola"]).unwrap_err().contains("--obs-dir"));
    }
}
