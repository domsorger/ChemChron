# ChemChron

[![Latest release](https://img.shields.io/github/v/release/domsorger/ChemChron)](https://github.com/domsorger/ChemChron/releases)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

Copyright 2026 Dominik Sorger, Department of Earth Sciences, University of Graz

ChemChron is one program for accessory-mineral geochronology (monazite,
xenotime, zircon and others). It reduces LA-ICP-MS U–Th–Pb and trace-element
data, calculates EPMA chemical Th–U–Pb ages, and brings both together,
analysis by analysis, for statistics, figures and export. It runs on Windows,
macOS and Linux.

## The four parts

| Part | What it does |
|---|---|
| **LA-ICP-MS** | Raw laser data to U–Th–Pb ratios, ages and trace-element contents |
| **EPMA** | Microprobe data to chemical Th–U–Pb ages and mineral formulae |
| **Combined results** | Both linked by analysis label: summaries, plots, long-term logs, export |
| **Settings** | Everything lab-wide: reference materials, decay constants, uncertainty models, criteria, colours |

Each part also works on its own.

## A typical workflow

1. **Load the laser data** (LA-ICP-MS > Data): a data folder or files,
   optionally with the laser log. Selections for every spot and baseline are
   made automatically from the log or the signal.
2. **Check the selections** (Signal & selections): adjust windows, cut out
   inclusions, sort spots into groups (reference materials and samples).
3. **Choose the corrections** (Corrections): primary reference material,
   baseline, down-hole fractionation, drift, common Pb, internal standard for
   trace elements.
4. **Look at the results**: ages and ratios, concordia, weighted means,
   trace elements and REE patterns, a spot map, and the quality checks of the
   secondary reference materials.
5. **Import the EPMA data** (EPMA > Data). Chemical ages, uncertainties and
   mineral formulae are calculated at once; reference materials are
   recognised by their codes in the labels.
6. **Send the laser results to Combined results** (LA-ICP-MS > Export). Laser
   spots are linked to EPMA analyses with the same label; spots without one
   become rows of their own.
7. **Evaluate and export** (Combined results): summaries per sample and
   reference material, chemical isochrons, weighted means, concordia
   diagrams, chemical vs U–Pb ages, age distributions, REE and composition
   plots. Figures are saved as PDF, SVG, EPS, PNG or TIFF, tables as Excel,
   ODS or CSV.
8. **Log the reference materials** (Combined results > Long-term logs). Session
   after session, they give the long-term reproducibility of the lab.

## Input data

- **LA-ICP-MS**: Thermo Element `.FIN2` files, Agilent MassHunter per-spot
  `.csv` files ("Intensity Vs Time"), generic time-resolved text tables with a
  time column, and ESI laser logs. Raw files are only read, never changed.
  Reduced U–Pb tables (final ratios with 2SE and ρ) and trace-element tables
  (element columns in ppm with 2SE and LOD) can be imported into Combined
  results.
- **EPMA**: microprobe exports with oxide (Mass%) columns and their error and
  detection-limit columns, ChemChron CSV tables, or tables pasted from a
  spreadsheet.
- A **synthetic demo dataset** is in [`testdata/epma/examples`](testdata/epma/examples);
  LA-ICP-MS > Data > Demo session opens a simulated project with a U–Pb session
  (primary 44069, secondaries MB508 and MI506), a trace-element session
  (primary NIST610, secondary NIST612) and EPMA analyses of the same spots.

## Main functions

**LA-ICP-MS**
- Automatic selections from the laser log or the signal; drawing and editing
  by hand; cut-outs of inclusions (by hand or automatically).
- Down-hole fractionation fit, normalisation to the primary reference material
  through time, common-Pb (²⁰⁴Pb, ²⁰⁷Pb, ²⁰⁸Pb methods) and ²³⁰Th corrections.
- Trace elements with internal standard (typed, per spot, or taken from the
  EPMA analysis of the same spot), detection limits, chondrite-normalised REE.
- Quality page: secondary reference materials against their accepted values.

**EPMA**
- Chemical ages from ThO₂, UO₂ and PbO with full uncertainty propagation.
- Mineral formulae and end-members; minerals recognised automatically.
- Several EPMA sessions per project, each with its own uncertainty estimate.

**Combined results**
- All results in one table, linked by label; editable groups, comments and sessions.
- Chemical isochrons, weighted means, concordia and discordia (Wetherill and
  Tera–Wasserburg), age distributions, REE and composition plots.
- Long-term logs of the reference materials (chemical and U–Pb).
- Export of tables, IsoplotR input and all figures of a project in one go.

**Uncertainties (the same in every part)**
- *Analytical*: the analysis itself.
- *Analytical + excess*: with the excess of the session that brings the
  scatter of its reference materials (primary and secondaries) to MSWD 1.
- *Total*: with the systematic term, i.e. the long-term reproducibility from
  the logs (once they hold 3 sessions), the primary's accepted value and,
  optionally, the decay constants.

## Methods and constants

- λ²³⁸U and λ²³⁵U after Jaffey et al. (1971); λ²³²Th after Le Roux &
  Glendenin (1963); ²³⁸U/²³⁵U = 137.818 (Hiess et al. 2012). All editable in
  Settings.
- Chemical isochrons (CHIME; Suzuki & Adachi 1991; Suzuki & Kato 2008;
  Suzuki & Dunkley 2014) by York
  regression with errors in both variables.
- Concordia ages after Ludwig (1998); discordia by York regression.
- Overdispersion as in IsoplotR (Vermeesch 2018): the uncertainty of a mean is
  enlarged by √MSWD only where the scatter is significant (p < 0.05).
- The uncertainty model is described in the program (About > Uncertainty
  calculations).

## Where things are stored

- **Project file** (`*.chemchron.json`): the analyses, the linked laser data
  and the laser sessions (raw data files are only referenced, never changed).
- **Lab settings** (shared by all projects): reference materials, decay
  constants, uncertainty models, criteria, colours and the long-term logs, in
  `~/.local/share/ChemChron/option_data` (Linux),
  `~/Library/Application Support/ChemChron/option_data` (macOS) or
  `%APPDATA%\ChemChron\option_data` (Windows). Settings > General > Set working
  directory… changes the settings folder.

## Download and system requirements

Ready-made programs are on the [Releases](https://github.com/domsorger/ChemChron/releases)
page of this repository:

| System | File |
|---|---|
| macOS 11 or later (Intel and Apple silicon, universal app) | `ChemChron-<version>-macOS.dmg` |
| Windows 10 or 11, 64-bit | `ChemChron-<version>-Windows-Setup.exe` (installer) or `-Windows-x64.zip` |
| Linux x86_64 or ARM64 (built on Ubuntu 22.04: distributions of that age or newer) | `ChemChron-<version>-Linux-<arch>.AppImage` or `.tar.gz` |

The Mac app is not notarised: open it the first time with right-click > Open.
Windows may show "unknown publisher" (More info > Run anyway).

## Building from source

ChemChron is written in Rust (https://rustup.rs). On Debian/Ubuntu first install

    sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev

then

    cargo build --release        # the program is target/release/chemchron
    cargo test                   # the tests

The helper scripts build an installed program: `sh macos/make_app.sh`
(ChemChron.app), `windows\make_windows.bat` (ChemChron.exe with a desktop
shortcut) and `sh linux/install.sh` (adds ChemChron to the applications menu).
Releases are built by the workflow in `.github/workflows/release.yml` when a
version tag (e.g. `v1.2.0`) is pushed.

## Citing ChemChron

A paper describing ChemChron is in preparation. Until then, please cite the
software itself: use "Cite this repository" on GitHub (from
[`CITATION.cff`](CITATION.cff)).

## Problems and suggestions

Please use the [GitHub Issues](https://github.com/domsorger/ChemChron/issues)
of this repository.

## License

ChemChron is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version (GPL-3.0-or-later). It is distributed in the hope that it will be
useful, but without any warranty. See the [`LICENSE`](LICENSE) file for details.

## Use of AI

The program code, its tests and this documentation were written with the AI
coding assistant Claude Code (Anthropic), working under the direction of the
author. The author set the scientific methods, the requirements and the
decisions (e.g. the uncertainty model), reviewed the results and tested the
program with real LA-ICP-MS and EPMA data. The calculations are checked by
automated tests, including regression tests against stored reference results
and tests against independently published values of reference materials. The
author is responsible for the content.
