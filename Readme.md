# ChemChron 1.2.0

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

1. **Load the laser data** (LA-ICP-MS > Data): a data folder or files (Thermo
   Element, Agilent, text tables), optionally with the laser log. Selections
   for every spot and baseline are made automatically from the log or the signal.
2. **Check the selections** (Signal & selections): adjust windows, cut out
   inclusions, sort spots into groups (reference materials and samples).
3. **Choose the corrections** (Corrections): primary reference material,
   baseline, down-hole fractionation, drift, common Pb, internal standard for
   trace elements.
4. **Look at the results**: ages and ratios, concordia, weighted means,
   trace elements and REE patterns, a spot map, and the quality checks of the
   secondary reference materials.
5. **Import the EPMA data** (EPMA > Data): microprobe exports or ChemChron
   tables. Chemical ages, uncertainties and mineral formulae are calculated
   at once; reference materials are recognised by their codes in the labels.
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
- Chemical isochrons (York regression), weighted means, concordia and discordia
  (Wetherill and Tera–Wasserburg), age distributions, REE and composition plots.
- Long-term logs of the reference materials (chemical and U–Pb).
- Export of tables, IsoplotR input and all figures of a project in one go.

**Uncertainties (the same in every part)**
- *Analytical*: the analysis itself.
- *Analytical + excess*: with the excess of the session that brings the
  scatter of its reference materials (primary and secondaries) to MSWD 1.
- *Total*: with the systematic term, i.e. the long-term reproducibility from
  the logs (once they hold 3 sessions), the primary's accepted value and,
  optionally, the decay constants.

## Where things are stored

- **Project file** (`*.chemchron.json`): the analyses, the linked laser data
  and the laser sessions (raw data files are only referenced, never changed).
- **Lab settings** (shared by all projects): reference materials, decay
  constants, uncertainty models, criteria, colours and the long-term logs, in
  `~/.local/share/ChemChron/option_data` (Linux),
  `~/Library/Application Support/ChemChron/option_data` (macOS) or
  `%APPDATA%\ChemChron\option_data` (Windows). Settings > General > Set working
  directory… to change the settings folder.

## Download

Ready-made programs are on the **Releases** page of this repository:

| System | File |
|---|---|
| macOS (Intel and Apple silicon) | `ChemChron-<version>-macOS.dmg` |
| Windows | `ChemChron-<version>-Windows-Setup.exe` (installer) or `-Windows-x64.zip` |
| Linux (x86_64, ARM) | `ChemChron-<version>-Linux-<arch>.AppImage` or `.tar.gz` |

The Mac app is not notarised: open it the first time with right-click > Open.
Windows may show "unknown publisher" (More info > Run anyway).

