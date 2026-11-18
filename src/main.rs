//! OBS Doctor: revisa tu PC y tu configuración de OBS y te dice, en simple,
//! qué cambiar para transmitir mejor. Solo lee: no modifica ni cierra nada.

mod advice;
mod hardware;
mod ini;
mod obs;
mod processes;
mod report;

use std::path::PathBuf;
use std::time::Duration;

use sysinfo::System;

/// How long we watch the machine before measuring CPU usage per program.
const SAMPLE_TIME: Duration = Duration::from_millis(1500);

fn main() {
    report::enable_colors();
    report::banner();

    let obs_dir_override = parse_obs_dir_arg();

    let mut sys = match System::new_all() {
        Ok(sys) => sys,
        Err(err) => {
            eprintln!("  No pude leer la información de esta PC: {err}");
            report::pause_if_double_clicked();
            std::process::exit(1);
        }
    };
    std::thread::sleep(SAMPLE_TIME);
    sys.refresh_all();

    let hw = hardware::read(&sys);
    let apps = processes::read(&sys);
    let obs_status = obs::read(obs_dir_override);

    let suggested = advice::suggest(&hw);
    let review = advice::review(&hw, &obs_status, &suggested);

    report::print(&hw, &obs_status, &suggested, &review, &apps);
    report::pause_if_double_clicked();
}

/// Supports `--obs-dir <carpeta>` for portable OBS installs.
fn parse_obs_dir_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--obs-dir" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
