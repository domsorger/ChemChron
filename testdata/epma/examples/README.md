# ChemChron demo dataset

A **synthetic** dataset for trying out ChemChron. Sample names, ages and
compositions are fictitious and not tied to any locality, study or published
analysis. The reference-material codes and their accepted chemical ages are
WC425 (425 ± 3 Ma, Aleinikoff et al. 2006) and MB508 (508 ± 15 Ma, Gonçalves
et al. 2016) from ChemChron's reference list, and DEMO-X320 (320 ± 20 Ma), a
xenotime code used only for this demo (add it in Settings > Reference
materials to see its checks); their isotopic ratios here are synthetic,
concordant at those ages.

The demo in the program itself (LA-ICP-MS > Data > Demo session) opens a
simulated project of the same kind: a U–Pb laser session (primary 44069,
secondaries MB508 and MI506), a trace-element laser session (primary NIST610,
secondary NIST612) and EPMA analyses of the same spots.

## Files

| File | Content |
|---|---|
| `demo_epma_S1.csv` | EPMA session 1: WC425 ×10, MB508 ×10, DEMO-A ×16, DEMO-B ×16 (monazite) |
| `demo_epma_S2.csv` | EPMA session 2: WC425 ×10, MB508 ×10, DEMO-X320 ×8 (xenotime), DEMO-C ×17 (monazite), DEMO-Cx ×12 (xenotime) |
| `demo_upb_laicpms.csv` | LA-ICP-MS U–Pb results: final ratios with 2SE(prop) and ρ, ²⁰⁸Pb/²³²Th: 44 spots |
| `demo_hree_laicpms.csv` | LA-ICP-MS Y and Tb–Lu (ppm) with 2SE(prop) and LOD: 30 spots |
| `demo_project.chemchron.json` | all of the above imported, sessions and sample groups set |

EPMA files are in the ChemChron CSV format with `X(Error%)` (1σ) and `X(D.L.)`
(ppm) columns, as exported from the microprobe software. Analyses are labelled
`<group>_<mnz|xtm><grain>_<spot>`; LA-ICP-MS spots carry the label of the EPMA
analysis on the same spot, reference spots the alternative codes `M44`
(→ WC425) and `MBA` (→ MB508).

## Samples and what they show

| Group | Mineral | Age | Features |
|---|---|---|---|
| DEMO-A | monazite | 1850 Ma | large Th spread (isochron); one older-core outlier (`DEMO-A_mnz05_2`); HREE-depleted; U–Pb: one imprecise spot (`mnz10_1`), two U–Pb-only grains |
| DEMO-B | monazite | 560 Ma | HREE-rich; U–Pb: recent Pb loss in four spots (discordant) → discordia anchored at 0 Ma |
| DEMO-C | monazite | 330 Ma | one Pb-loss outlier (`mnz06_1`); one low-Th analysis with PbO below d.l. (`mnz09_1`); U–Pb: one common-Pb spot (`mnz04_1`) |
| DEMO-Cx | xenotime | 330 Ma | same rock as DEMO-C; U-rich, low Th; HREE pattern of xenotime |

Chemical ages carry a spot-to-spot excess scatter of 1.5 % (session 1) and
3.5 % (session 2, 2σ) on top of the counting errors. The U–Pb ratios carry a
small extra ²³⁸U/²⁰⁶Pb scatter (≈ 1 %, 2σ). Lu is below its LOD in some DEMO-A
spots.

## Trying it

1. **EPMA** > Data > Import EPMA data → `demo_epma_S1.csv`, then Append EPMA
   data → `demo_epma_S2.csv` (or Project > Open project →
   `demo_project.chemchron.json` for everything at once).
2. **Combined results** > Data > Import laser results from files →
   `demo_upb_laicpms.csv` and `demo_hree_laicpms.csv`.
3. **Combined results**: Summary, chemical isochron, weighted mean, concordia
   (discordia for DEMO-B), chemical vs U–Pb ages, age distribution, REE and
   composition plots; Export for tables and figures.
4. **Settings** > Uncertainty models: the random excess per session and the
   systematic term; Combined results > Long-term logs to log the reference
   materials.
