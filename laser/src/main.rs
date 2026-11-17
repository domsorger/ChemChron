// SPDX-License-Identifier: GPL-3.0-or-later
//! ChemChron LA-ICP-MS part on its own: LA-ICP-MS data reduction from raw sweeps to traceable results.
//!
//! `--screenshot <file.png>` starts normally, draws about 120 frames, saves the
//! window as a PNG and exits. `--page <name>` opens a page first and `--open
//! <path>` opens a session or data folder, both useful with `--screenshot`.

use std::path::PathBuf;
use chemchron_laser::app::{Page, TraceApp};

struct Args {
    screenshot: Option<PathBuf>,
    page: Option<Page>,
    open: Vec<PathBuf>,
    size: (f32, f32),
    light: bool,
    select: Option<String>,
    strips: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        screenshot: None,
        page: None,
        open: Vec::new(),
        size: (1440.0, 900.0),
        light: false,
        select: None,
        strips: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => {
                args.screenshot = Some(PathBuf::from(it.next().ok_or("--screenshot needs a file")?));
            }
            "--page" => {
                let name = it.next().ok_or("--page needs a name")?;
                args.page = Some(Page::from_name(&name).ok_or(format!("Unknown page \"{name}\""))?);
            }
            "--open" => args.open.push(PathBuf::from(it.next().ok_or("--open needs a path")?)),
            "--light" => args.light = true,
            "--select" => args.select = Some(it.next().ok_or("--select needs a selection label")?),
            "--strips" => args.strips = true,
            "--size" => {
                let v = it.next().ok_or("--size needs WIDTHxHEIGHT")?;
                let (w, h) = v.split_once('x').ok_or("--size needs WIDTHxHEIGHT")?;
                args.size = (
                    w.parse().map_err(|_| "--size needs numbers")?,
                    h.parse().map_err(|_| "--size needs numbers")?,
                );
            }
            other => return Err(format!("Unknown option \"{other}\"")),
        }
    }
    Ok(args)
}

fn main() -> eframe::Result {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if args.screenshot.is_some() {
        // A hidden or stalled window must not hang an automated run forever.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(180));
            eprintln!("Timed out before the picture was saved.");
            std::process::exit(1);
        });
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ChemChron LA-ICP-MS")
            .with_inner_size([args.size.0, args.size.1])
            .with_min_inner_size([1180.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ChemChron LA-ICP-MS",
        options,
        Box::new(move |cc| {
            let mut app = TraceApp::new(&cc.egui_ctx);
            app.dark = !args.light;
            if let Some(path) = args.screenshot {
                app.request_screenshot(path);
            }
            if !args.open.is_empty() {
                app.open_blocking(&args.open);
            }
            if let Some(label) = &args.select {
                app.selected = app.session.selections.iter().find(|s| &s.label == label).map(|s| s.id);
                app.zoom_to_selected();
            }
            if args.strips {
                use chemchron_laser_core::drs::{upb::RatioKind, SeriesKey};
                app.signal.picks.extend([
                    SeriesKey::TotalBeam,
                    SeriesKey::RatioRaw(RatioKind::R68),
                    SeriesKey::RatioFinal(RatioKind::R76),
                    SeriesKey::Age(RatioKind::R68),
                ]);
            }
            if let Some(page) = args.page {
                app.page = page;
            }
            Ok(Box::new(app))
        }),
    )
}
