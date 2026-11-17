// SPDX-License-Identifier: GPL-3.0-or-later
//! ChemChron LA-ICP-MS calculation core: importers, data model, interpolation, trace
//! element and U-Th-Pb reduction, QA/QC and export. The user interface calls
//! this crate; this crate never calls the user interface.

pub mod autosel;
pub mod drs;
pub mod export;
pub mod geochron;
pub mod interp;
pub mod io;
pub mod model;
pub mod oxides;
pub mod project;
pub mod refmat;
pub mod stats;
pub mod svg;
pub mod synthetic;
pub mod testdata;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
