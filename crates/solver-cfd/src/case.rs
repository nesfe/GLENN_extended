use glam::DVec3;
use glenn_core::mesh::SolidMesh;
use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, fs, path::Path};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowModel {
    Laminar,
    SteadyRans,
    UnsteadyRans,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Preview,
    Standard,
    Fine,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub model: FlowModel,
    pub resolution: Resolution,
    pub speed: f64,
    pub density: f64,
    pub kinematic_viscosity: f64,
    pub alpha_degrees: f64,
    pub iterations: usize,
    /// Physical time in root-length/free-stream transit times for URANS.
    pub transit_times: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            model: FlowModel::SteadyRans,
            resolution: Resolution::Preview,
            speed: 20.0,
            density: 1.225,
            kinematic_viscosity: 1.5e-5,
            alpha_degrees: 0.0,
            iterations: 500,
            transit_times: 10.0,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        for (name, x, lo, hi) in [
            ("Скорость, м/с", self.speed, 0.001, 100.0),
            ("Плотность, кг/м³", self.density, 0.001, 20000.0),
            (
                "Кинематическая вязкость, м²/с",
                self.kinematic_viscosity,
                1e-9,
                1.0,
            ),
            ("Угол, град", self.alpha_degrees, -90.0, 90.0),
            ("Время в L/V", self.transit_times, 0.1, 1000.0),
        ] {
            if !x.is_finite() || !(lo..=hi).contains(&x) {
                return Err(format!("{name}: допустимо {lo}…{hi}"));
            }
        }
        if !(10..=100000).contains(&self.iterations) {
            return Err("Число итераций: 10…100000".into());
        }
        Ok(())
    }
    pub fn velocity(&self) -> DVec3 {
        let a = self.alpha_degrees.to_radians();
        DVec3::new(a.cos(), a.sin(), 0.0) * self.speed
    }
}
fn header(class: &str, name: &str) -> String {
    format!("FoamFile {{ version 2.0; format ascii; class {class}; object {name}; }}\n")
}
fn write(dir: &Path, name: &str, contents: String) -> Result<(), String> {
    fs::write(dir.join(name), contents).map_err(|e| format!("{name}: {e}"))
}
fn vec(v: DVec3) -> String {
    format!("({:.12e} {:.12e} {:.12e})", v.x, v.y, v.z)
}
/// Fresh case only. Units are metres, seconds, kg. User files are never overwritten.
pub fn write_case(dir: &Path, mesh: &SolidMesh, cfg: &Config) -> Result<(), String> {
    cfg.validate()?;
    let mut checked = mesh.clone();
    let report = checked.validate_and_orient()?;
    if dir.exists() {
        return Err("Каталог расчёта уже существует; выберите новый".into());
    }
    for d in ["0", "constant/triSurface", "system"] {
        fs::create_dir_all(dir.join(d)).map_err(|e| e.to_string())?;
    }
    write(dir, "constant/triSurface/body.stl", checked.ascii_stl())?;
    write(
        dir,
        "glenn-case.json",
        serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?,
    )?;
    let l = (report.max - report.min).max_element();
    let center = (report.max + report.min) * 0.5;
    let lo = report.min - DVec3::new(3.0, 3.0, 3.0) * l;
    let hi = report.max + DVec3::new(8.0, 3.0, 3.0) * l;
    let base = match cfg.resolution {
        Resolution::Preview => 3.0,
        Resolution::Standard => 5.0,
        Resolution::Fine => 8.0,
    };
    let cells = ((hi - lo) / l * base).ceil().as_uvec3();
    let vertices = [
        lo,
        DVec3::new(hi.x, lo.y, lo.z),
        DVec3::new(hi.x, hi.y, lo.z),
        DVec3::new(lo.x, hi.y, lo.z),
        DVec3::new(lo.x, lo.y, hi.z),
        DVec3::new(hi.x, lo.y, hi.z),
        hi,
        DVec3::new(lo.x, hi.y, hi.z),
    ];
    let points = vertices.map(vec).join("\n");
    write(
        dir,
        "system/blockMeshDict",
        header("dictionary", "blockMeshDict")
            + &format!(
                r#"
convertToMeters 1;
vertices ({points});
blocks (hex (0 1 2 3 4 5 6 7) ({} {} {}) simpleGrading (1 1 1));
edges ();
boundary (
 inlet {{ type patch; faces ((0 4 7 3)); }}
 outlet {{ type patch; faces ((1 2 6 5)); }}
 farfield {{ type patch; faces ((0 1 5 4) (3 7 6 2) (0 3 2 1) (4 5 6 7)); }}
);
mergePatchPairs ();
"#,
                cells.x, cells.y, cells.z
            ),
    )?;
    // A span-based level erased thin wings entirely: no cell centre remained
    // inside them. Resolve the solid's volume/area thickness, independently of
    // its span, before retaining the outside connected region.
    let thickness =
        (2.0 * report.volume / report.area).min((report.max - report.min).min_element());
    let across = match cfg.resolution {
        Resolution::Preview => 2.0,
        Resolution::Standard => 3.0,
        Resolution::Fine => 4.0,
    };
    let target = (thickness / across).min(l / (16.0 * across));
    let base_cell = ((hi - lo) / cells.as_dvec3()).max_element();
    let refinement = (base_cell / target).log2().ceil().max(1.0) as u32;
    if refinement > 10 {
        return Err("Геометрия слишком тонкая для автоматической сетки: требуется специальная настройка сетки".into());
    }
    let layers = if cfg.resolution == Resolution::Preview {
        0
    } else {
        3
    };
    let location = vec(lo + DVec3::splat(l * 0.5));
    write(
        dir,
        "system/snappyHexMeshDict",
        header("dictionary", "snappyHexMeshDict")
            + &format!(
                r#"
castellatedMesh true;
snap true;
addLayers {};
geometry {{ body.stl {{ type triSurfaceMesh; name body; }} }}
castellatedMeshControls {{
 maxLocalCells 2000000; maxGlobalCells 2000000; minRefinementCells 0;
 maxLoadUnbalance 0.10; nCellsBetweenLevels 3;
 features ();
 refinementSurfaces {{ body {{ level ({refinement} {refinement}); patchInfo {{ type wall; }} }} }}
 resolveFeatureAngle 30;
 refinementRegions {{ }}
 locationInMesh {location}; allowFreeStandingZoneFaces true;
}}
snapControls {{ nSmoothPatch 3; tolerance 2.0; nSolveIter 30; nRelaxIter 5; nFeatureSnapIter 10; implicitFeatureSnap true; explicitFeatureSnap false; multiRegionFeatureSnap false; }}
addLayersControls {{
 relativeSizes true; layers {{ body {{ nSurfaceLayers {layers}; }} }}
 expansionRatio 1.2; finalLayerThickness 0.3; minThickness 0.1;
 nGrow 0; featureAngle 60; nRelaxIter 5; nSmoothSurfaceNormals 1;
 nSmoothNormals 3; nSmoothThickness 10; maxFaceThicknessRatio 0.5;
 maxThicknessToMedialRatio 0.3; minMedialAxisAngle 90;
 nBufferCellsNoExtrude 0; nLayerIter 30;
}}
meshQualityControls {{
 maxNonOrtho 65; maxBoundarySkewness 3.5; maxInternalSkewness 3.5;
 maxConcave 80; minVol 1e-30; minTetQuality 1e-15;
 minArea -1; minTwist 0.02; minDeterminant 0.001;
 minFaceWeight 0.02; minVolRatio 0.01; minTriangleTwist -1;
 nSmoothScale 4; errorReduction 0.75;
}}
mergeTolerance 1e-6;
"#,
                if layers > 0 { "true" } else { "false" }
            ),
    )?;
    write(
        dir,
        "constant/transportProperties",
        header("dictionary", "transportProperties")
            + &format!(
                "transportModel Newtonian;\nnu [0 2 -1 0 0 0 0] {};\n",
                cfg.kinematic_viscosity
            ),
    )?;
    write(
        dir,
        "constant/turbulenceProperties",
        header("dictionary", "turbulenceProperties")
            + if cfg.model == FlowModel::Laminar {
                "simulationType laminar;\n"
            } else {
                "simulationType RAS;\nRAS { RASModel kOmegaSST; turbulence on; printCoeffs on; }\n"
            },
    )?;
    let u = vec(cfg.velocity());
    let boundary = |wall: &str, far: &str, out: &str| {
        format!(
            "boundaryField {{ inlet {{ {far} }} farfield {{ {far} }} outlet {{ {out} }} body {{ {wall} }} }}\n"
        )
    };
    write(
        dir,
        "0/U",
        header("volVectorField", "U")
            + &format!("dimensions [0 1 -1 0 0 0 0]; internalField uniform {u};\n")
            + &boundary(
                "type noSlip;",
                &format!("type freestreamVelocity; freestreamValue uniform {u};"),
                &format!("type freestreamVelocity; freestreamValue uniform {u};"),
            ),
    )?;
    write(
        dir,
        "0/p",
        header("volScalarField", "p")
            + "dimensions [0 2 -2 0 0 0 0]; internalField uniform 0;\n"
            + &boundary(
                "type zeroGradient;",
                "type freestreamPressure; freestreamValue uniform 0;",
                "type freestreamPressure; freestreamValue uniform 0;",
            ),
    )?;
    if cfg.model != FlowModel::Laminar {
        let k = 1.5 * (0.01 * cfg.speed).powi(2);
        let omega = k.sqrt() / (0.09_f64.powf(0.25) * 0.07 * l);
        for (name, dims, value, wall) in [
            ("k", "0 2 -2", k, "kqRWallFunction"),
            ("omega", "0 0 -1", omega, "omegaWallFunction"),
            ("nut", "0 2 -1", 0.0, "nutkWallFunction"),
        ] {
            let far = if name == "nut" {
                "type calculated; value uniform 0;".to_string()
            } else {
                format!("type inletOutlet; inletValue uniform {value}; value uniform {value};")
            };
            write(
                dir,
                &format!("0/{name}"),
                header("volScalarField", name)
                    + &format!("dimensions [{dims} 0 0 0 0]; internalField uniform {value};\n")
                    + &boundary(&format!("type {wall}; value uniform {value};"), &far, &far),
            )?;
        }
    }
    let transient = cfg.model == FlowModel::UnsteadyRans;
    let solver = if transient {
        "pimpleFoam"
    } else {
        "simpleFoam"
    };
    let end = if transient {
        cfg.transit_times * l / cfg.speed
    } else {
        cfg.iterations as f64
    };
    let dt = if transient {
        l / cfg.speed * 0.002
    } else {
        1.0
    };
    let write_control = if transient {
        "adjustableRunTime"
    } else {
        "timeStep"
    };
    let interval = if transient {
        end / 10.0
    } else {
        (cfg.iterations / 10).max(1) as f64
    };
    write(
        dir,
        "system/controlDict",
        header("dictionary", "controlDict")
            + &format!(
                r#"
application {solver}; startFrom startTime; startTime 0; stopAt endTime;
endTime {end}; deltaT {dt}; writeControl {write_control}; writeInterval {interval};
purgeWrite 2; writeFormat ascii; writePrecision 10; writeCompression off;
timeFormat general; timePrecision 8; runTimeModifiable false;
adjustTimeStep {}; maxCo 0.5; maxDeltaT {};
functions {{
 loads {{ type forces; libs ("libforces.so"); patches (body); rho rhoInf; rhoInf {}; CofR {}; log true; writeControl timeStep; writeInterval 1; }}
 vort {{ type vorticity; libs ("libfieldFunctionObjects.so"); field U; result vorticity; executeControl writeTime; writeControl writeTime; }}
 yPlus {{ type yPlus; libs ("libfieldFunctionObjects.so"); executeControl writeTime; writeControl writeTime; }}
}}
"#,
                if transient { "true" } else { "false" },
                l / cfg.speed * 0.02,
                cfg.density,
                vec(center)
            ),
    )?;
    write(
        dir,
        "system/fvSchemes",
        header("dictionary", "fvSchemes")
            + &format!(
                r#"
ddtSchemes {{ default {}; }}
gradSchemes {{ default cellLimited Gauss linear 1; }}
divSchemes {{ default none; div(phi,U) bounded Gauss linearUpwind grad(U); div(phi,k) bounded Gauss upwind; div(phi,omega) bounded Gauss upwind; div((nuEff*dev2(T(grad(U))))) Gauss linear; }}
laplacianSchemes {{ default Gauss linear limited 0.5; }}
interpolationSchemes {{ default linear; }}
snGradSchemes {{ default limited 0.5; }}
wallDist {{ method meshWave; }}
"#,
                if transient { "Euler" } else { "steadyState" }
            ),
    )?;
    write(
        dir,
        "system/fvSolution",
        header("dictionary", "fvSolution")
            + r#"
solvers {
 p { solver GAMG; tolerance 1e-8; relTol 0.05; smoother GaussSeidel; }
 pFinal { $p; relTol 0; }
 "(U|k|omega)" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0.1; }
 "(U|k|omega)Final" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0; }
}
SIMPLE { nNonOrthogonalCorrectors 1; consistent yes; residualControl { p 1e-5; U 1e-6; "(k|omega)" 1e-6; } }
PIMPLE { nOuterCorrectors 2; nCorrectors 2; nNonOrthogonalCorrectors 1; }
relaxationFactors { fields { p 0.3; } equations { U 0.7; k 0.7; omega 0.7; } }
"#,
    )?;
    // Sample a regular cloud for a shared 3D interpolator and slice display.
    // Fluid cells outside mesh and inside solids remain explicitly missing.
    // Resolve the model neighbourhood, not an enormous span-sized empty box.
    // This cloud is for display only; it does not alter finite-volume cells.
    let chord = (report.max.x - report.min.x).max(thickness);
    let padding = DVec3::new(0.5 * chord, 0.3 * chord, 0.15 * chord);
    let sample_lo = report.min - padding;
    let sample_hi = report.max + DVec3::new(1.5 * chord, padding.y, padding.z);
    let dims = match cfg.resolution {
        Resolution::Preview => [121usize, 65, 49],
        Resolution::Standard => [161, 81, 65],
        Resolution::Fine => [201, 97, 81],
    };
    write(
        dir,
        "body-bounds.json",
        serde_json::to_string(&(report.min.to_array(), report.max.to_array())).unwrap(),
    )?;
    let mut locations = String::new();
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let f = DVec3::new(
                    x as f64 / (dims[0] - 1) as f64,
                    y as f64 / (dims[1] - 1) as f64,
                    z as f64 / (dims[2] - 1) as f64,
                );
                writeln!(
                    locations,
                    "{}",
                    vec(sample_lo + (sample_hi - sample_lo) * f)
                )
                .unwrap();
            }
        }
    }
    write(
        dir,
        "sample-grid.json",
        serde_json::to_string(&(dims, sample_lo.to_array(), sample_hi.to_array())).unwrap(),
    )?;
    write(
        dir,
        "system/sampleDict",
        header("dictionary", "sampleDict")
            + &format!(
                r#"
functions {{ sample {{ type sets; libs ("libsampling.so"); writeControl writeTime;
setFormat raw; interpolationScheme cellPoint; fields (U p vorticity);
sets (volume {{ type cloud; axis xyz; points ({locations}); }});
}} }}
"#
            ),
    )?;
    Ok(())
}
