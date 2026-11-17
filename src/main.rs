// SPDX-License-Identifier: GPL-3.0-or-later
//! ChemChron: LA-ICP-MS reduction (Laser), EPMA chemical Th–U–Pb dating
//! (EPMA), and both together (Combine), in one window.
//!
//! `--part laser|epma|combine`, `--page <name>`, `--open <project, laser
//! session or data folder>`, `--epma <file.csv>` (import), `--laser <folder>`,
//! `--link` (link the laser results), `--light`, `--size WxH` and
//! `--screenshot <file.png>` (draws, saves a picture, exits), `--demo` (the
//! demo data of all parts) and `--screenshot-tour <folder>` (a picture of
//! every page, dark and light, then exits).
//! `--take-over-settings <option_data folder>` and `--import-reference-files
//! <folder>` update the lab settings without opening a window.

// no console window behind the program on Windows
#![cfg_attr(windows, windows_subsystem = "windows")]

use chemchron::{app, combine, epma, global};

use std::path::PathBuf;

#[derive(Default)]
struct Args {
    screenshot: Option<PathBuf>,
    tour: Option<PathBuf>,
    demo: bool,
    part: Option<app::Part>,
    page: Option<String>,
    open: Vec<PathBuf>,
    epma: Vec<PathBuf>,
    laser: Vec<PathBuf>,
    link: bool,
    light: bool,
    size: Option<(f32, f32)>,
    take_over: Option<PathBuf>,
    write_icon: Option<PathBuf>,
    reference_files: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--screenshot-tour" => a.tour = Some(PathBuf::from(it.next().ok_or("--screenshot-tour needs a folder")?)),
            "--demo" => a.demo = true,
            "--screenshot" => a.screenshot = Some(PathBuf::from(it.next().ok_or("--screenshot needs a file")?)),
            "--part" => {
                let n = it.next().ok_or("--part needs laser, epma or combine")?;
                a.part = Some(app::Part::from_name(&n).ok_or(format!("Unknown part \"{n}\""))?);
            }
            "--page" => a.page = Some(it.next().ok_or("--page needs a name")?),
            "--open" => a.open.push(PathBuf::from(it.next().ok_or("--open needs a path")?)),
            "--epma" => a.epma.push(PathBuf::from(it.next().ok_or("--epma needs a file")?)),
            "--laser" => a.laser.push(PathBuf::from(it.next().ok_or("--laser needs a folder")?)),
            "--link" => a.link = true,
            "--write-icon" => a.write_icon = Some(PathBuf::from(it.next().ok_or("--write-icon needs a file")?)),
            "--take-over-settings" => a.take_over = Some(PathBuf::from(it.next().ok_or("--take-over-settings needs an option_data folder")?)),
            "--import-reference-files" => a.reference_files = Some(PathBuf::from(it.next().ok_or("--import-reference-files needs a folder")?)),
            "--light" => a.light = true,
            "--size" => {
                let v = it.next().ok_or("--size needs WIDTHxHEIGHT")?;
                let (w, h) = v.split_once('x').ok_or("--size needs WIDTHxHEIGHT")?;
                a.size = Some((w.parse().map_err(|_| "--size needs numbers")?, h.parse().map_err(|_| "--size needs numbers")?));
            }
            other => return Err(format!("Unknown option \"{other}\"")),
        }
    }
    Ok(a)
}

fn main() -> eframe::Result {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Some(p) = &args.write_icon {
        // the icon as a 1024-pixel PNG (macos/make_app.sh turns it into the Mac icon)
        match chemchron::icon::write_png(p, 1024) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    if args.take_over.is_some() || args.reference_files.is_some() {
        std::process::exit(update_settings(&args));
    }
    if args.screenshot.is_some() || args.tour.is_some() {
        let secs = if args.tour.is_some() { 900 } else { 240 };
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            eprintln!("Timed out before the picture was saved.");
            std::process::exit(1);
        });
    }
    let size = args.size.unwrap_or((1480.0, 920.0));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ChemChron").with_icon(std::sync::Arc::new(chemchron::icon::icon_data())).with_drag_and_drop(true).with_inner_size([size.0, size.1]).with_min_inner_size([1180.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ChemChron",
        options,
        Box::new(move |cc| {
            let mut a = app::ChemApp::new(&cc.egui_ctx);
            a.dark = !args.light;
            if let Some(p) = args.screenshot {
                a.request_screenshot(p);
            }
            if args.demo {
                chemchron::demo::load(&mut a);
            }
            for p in &args.open {
                if p.is_dir() {
                    a.laser.open_blocking(std::slice::from_ref(p));
                    a.part = app::Part::Laser;
                } else {
                    a.open_project(p);
                }
            }
            if !args.laser.is_empty() {
                a.laser.open_blocking(&args.laser);
            }
            if !args.epma.is_empty() {
                epma::import_files(&mut a, args.epma.clone(), false);
            }
            if args.link {
                combine::link_laser_now(&mut a);
            }
            if let Some(p) = args.part {
                a.part = p;
            }
            if let Some(name) = &args.page {
                match a.part {
                    app::Part::Epma => {
                        if let Some(p) = epma::Page::from_name(name) {
                            a.epma.page = p;
                        }
                    }
                    app::Part::Combine => {
                        if let Some(p) = combine::Page::from_name(name) {
                            a.combine.page = p;
                        }
                    }
                    app::Part::Laser => {
                        if let Some(p) = chemchron_laser::app::Page::from_name(name) {
                            a.laser.page = p;
                        }
                    }
                    app::Part::Settings => {
                        if let Some(p) = global::Page::from_name(name) {
                            a.global.page = p;
                        }
                    }
                }
            }
            if let Some(d) = &args.tour {
                if let Err(e) = std::fs::create_dir_all(d) {
                    eprintln!("{}: {e}", d.display());
                    std::process::exit(1);
                }
                chemchron::tour::start(&mut a, d.clone());
            }
            Ok(Box::new(a))
        }),
    )
}

/// Takes over the settings of another settings folder and merges reference-material files into the lab list.
fn update_settings(args: &Args) -> i32 {
    let mut s = chemchron_core::settings::Settings::load();
    println!("Settings folder: {}", s.dir.display());
    // a copy of the folder as it was, next to it
    if s.dir.is_dir() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let backup = s.dir.with_file_name(format!("option_data_backup_{stamp}"));
        if std::fs::create_dir_all(&backup).is_ok() {
            if let Ok(rd) = std::fs::read_dir(&s.dir) {
                for e in rd.flatten() {
                    if e.path().is_file() {
                        let _ = std::fs::copy(e.path(), backup.join(e.file_name()));
                    }
                }
            }
            println!("Backup of the previous settings: {}", backup.display());
        }
    }
    if let Some(dir) = &args.take_over {
        let dir = if dir.join("option_data").is_dir() { dir.join("option_data") } else { dir.clone() };
        match chemchron_core::workdir::import_from(&mut s, &dir) {
            Ok(m) => println!("{m}"),
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
    }
    // the laser part's built-in starting set joins the list too (only what is missing)
    let builtin = chemchron_laser_core::refmat::builtin(&chemchron_laser_core::geochron::DecayConstants::default());
    let codes = s.label_codes();
    chemchron::refsync::merge_into_lab(&mut s.refs_nested, &builtin, &codes, true);
    if let Some(dir) = &args.reference_files {
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json"))).collect(),
            Err(e) => {
                eprintln!("{}: {e}", dir.display());
                return 1;
            }
        };
        files.sort();
        let (added, merged, bad) = chemchron::refsync::import_reference_files(&mut s.refs_nested, &files);
        println!("{}", chemchron::refs_page::import_message(&added, &merged, &bad));
        println!("New: {}", added.join(", "));
        println!("Values added to: {}", merged.join(", "));
    }
    match s.save() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
