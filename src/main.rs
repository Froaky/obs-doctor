//! OBS Doctor: revisa tu PC, tu internet y tu configuración de OBS y te dice,
//! en simple, qué cambiar para transmitir mejor.
//!
//! Solo lee: no modifica OBS ni Windows, y no cierra ningún programa.
//!
//! How the code is laid out: each area gathers facts into its own type
//! (`hardware`, `processes`, `obs::config`, `obs::logs`, `obs::scenes`,
//! `system`, `net`), [`facts::Facts`] holds them all, the rules of each area
//! turn facts into [`model::Advice`], and `report` draws the result.

mod cli;
mod diagnosis;
mod facts;
mod hardware;
mod ini;
mod model;
mod net;
mod obs;
mod platforms;
mod processes;
mod report;
mod suggest;
mod system;

use std::process::ExitCode;

use sysinfo::System;

use cli::{Command, Options};
use facts::Facts;
use report::console;

fn main() -> ExitCode {
    console::enable_colors();

    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(Command::Run(options)) => options,
        Ok(Command::Help) => {
            print!("{}", cli::HELP);
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("obs-doctor {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            console::problem(&message);
            return ExitCode::from(2);
        }
    };

    console::banner();
    let Some(facts) = gather(&options) else {
        console::pause_if_double_clicked();
        return ExitCode::FAILURE;
    };
    let data = report::build(&facts);
    show(&data, &options);
    console::pause_if_double_clicked();
    ExitCode::SUCCESS
}

/// Looks at everything, one area at a time, telling the streamer what is going on.
fn gather(options: &Options) -> Option<Facts> {
    console::step("Mirando tu PC y lo que tienes abierto...");
    let mut sys = match System::new_all() {
        Ok(sys) => sys,
        Err(err) => {
            console::problem(&format!("No pude leer la información de esta PC: {err}"));
            return None;
        }
    };
    let hw = hardware::read(&sys);
    let apps = processes::read(&mut sys);

    console::step("Leyendo tu OBS...");
    let paths = obs::locate(options.obs_dir.clone());
    let obs_status = obs::config::read(&paths);
    let logs = obs::logs::read(&paths);
    let scenes = obs::scenes::read(&paths);

    console::step("Revisando Windows...");
    let system_facts = system::read();

    let net_facts = if options.skip_network {
        net::skipped()
    } else {
        console::step("Revisando tu internet...");
        net::read(&mut |line| console::detail(line))
    };

    let mut facts = Facts {
        hw,
        apps,
        obs: obs_status,
        logs,
        scenes,
        system: system_facts,
        net: net_facts,
        suggested: suggest::Suggested::default(),
    };
    facts.suggested = suggest::suggest(&facts);
    Some(facts)
}

fn show(data: &model::ReportData, options: &Options) {
    if options.console_only {
        console::print_full(data);
        return;
    }
    let path = options.output.clone().unwrap_or_else(report::default_path);
    match report::write_html(data, &path) {
        Ok(()) => {
            let opened = !options.no_open && report::open_in_browser(&path);
            console::print_summary(data, &path, opened);
        }
        Err(err) => {
            console::problem(&format!(
                "No pude guardar el reporte en {} ({err}). Te lo muestro aquí:",
                console::shown_path(&path)
            ));
            console::print_full(data);
        }
    }
}
