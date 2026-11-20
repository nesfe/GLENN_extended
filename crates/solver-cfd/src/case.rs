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
    /// Free-stream Kelvin for compressible dry air. Older cases use kinematic pressure.
    #[serde(default)]
    pub air_temperature: Option<f64>,
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
            air_temperature: None,
            kinematic_viscosity: 1.5e-5,
            alpha_degrees: 0.0,
            iterations: 500,
            transit_times: 10.0,
        }
    }
}
impl Config {
    pub fn sound_speed(temperature: f64) -> f64 {
        (1.4 * Self::GAS_CONSTANT * temperature).sqrt()
    }
    const GAS_CONSTANT: f64 = 8314.46261815324 / 28.97;
    pub fn reference_pressure(&self) -> Option<f64> {
        self.air_temperature
            .map(|t| self.density * Self::GAS_CONSTANT * t)
    }
    pub fn gauge_pressure(&self, p: f64) -> f64 {
        self.reference_pressure()
            .map_or(p * self.density, |reference| p - reference)
    }
    pub fn solver(&self) -> &'static str {
        match (
            self.air_temperature.is_some(),
            self.model == FlowModel::UnsteadyRans,
        ) {
            (false, false) => "simpleFoam",
            (false, true) => "pimpleFoam",
            (true, false) => "rhoSimpleFoam",
            (true, true) => "rhoCentralFoam",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let max_speed = if let Some(t) = self.air_temperature {
            if !t.is_finite() || !(150.0..=500.0).contains(&t) {
                return Err("Температура воздуха должна быть от 150 до 500 К".into());
            }
            0.95 * Self::sound_speed(t)
        } else {
            100.0
        };
        for (name, x, lo, hi) in [
            ("Скорость, м/с", self.speed, 0.001, max_speed),
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
    pub fn coefficients(&self, force: DVec3, area: f64) -> Option<(f64, f64)> {
        if !area.is_finite() || area <= 0. {
            return None;
        }
        let (lift, drag) = self.wind_forces(force);
        let qa = 0.5 * self.density * self.speed.powi(2) * area;
        Some((lift / qa, drag / qa))
    }
    pub fn wind_direction(&self) -> DVec3 {
        let (sin, cos) = self.alpha_degrees.to_radians().sin_cos();
        DVec3::new(cos, sin, 0.0)
    }
    /// Lift and drag in newtons in the inlet wind axes.
    pub fn wind_forces(&self, force: DVec3) -> (f64, f64) {
        let wind = self.wind_direction();
        (wind.x * force.y - wind.y * force.x, wind.dot(force))
    }
    pub fn velocity(&self) -> DVec3 {
        self.wind_direction() * self.speed
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
    write_case_for_platform(dir, mesh, cfg, cfg!(windows))
}

pub(crate) fn write_case_for_platform(
    dir: &Path,
    mesh: &SolidMesh,
    cfg: &Config,
    native_windows: bool,
) -> Result<(), String> {
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
    // Size the first prism in metres from a flat-plate friction estimate.
    // OpenFOAM evaluates the resulting yPlus from the solved wall stress.
    let surface_cell = base_cell / 2.0_f64.powi(refinement as i32);
    let (sin, cos) = cfg.alpha_degrees.to_radians().sin_cos();
    let extent = report.max - report.min;
    let chord = extent.x * cos.abs() + extent.y * sin.abs();
    let re = cfg.speed * chord / cfg.kinematic_viscosity;
    let first_height = if cfg.model == FlowModel::Laminar {
        5.0 * chord / (12.0 * re.sqrt())
    } else {
        let skin_friction = 0.026 * re.powf(-1.0 / 7.0);
        let friction_speed = cfg.speed * (0.5 * skin_friction).sqrt();
        2.0 * 30.0 * cfg.kinematic_viscosity / friction_speed
    }
    .min(0.3 * surface_cell);
    let growth = 1.25_f64;
    let layers = match cfg.resolution {
        Resolution::Preview => 0,
        quality => (((0.3 * surface_cell / first_height).ln() / growth.ln()).ceil() as usize + 1)
            .clamp(3, if quality == Resolution::Fine { 24 } else { 18 }),
    };
    let min_thickness = first_height * 0.2;
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
 relativeSizes false; layers {{ body {{ nSurfaceLayers {layers}; }} }}
 expansionRatio {growth}; firstLayerThickness {first_height:e}; minThickness {min_thickness:e};
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
    let reference_pressure = cfg.reference_pressure().unwrap_or(0.0);
    let transient_gas = cfg.air_temperature.is_some() && cfg.model == FlowModel::UnsteadyRans;
    let pressure_dims = if cfg.air_temperature.is_some() {
        "1 -1 -2"
    } else {
        "0 2 -2"
    };
    let far_pressure =
        format!("type freestreamPressure; freestreamValue uniform {reference_pressure};");
    let out_pressure = if transient_gas {
        // The MinGW object registry sanitizes scoped field names for Windows.
        // WSL runs the Linux binary even when GLENN itself runs on Windows.
        let psi = if native_windows {
            "thermo_psi"
        } else {
            "thermo:psi"
        };
        format!(
            "type waveTransmissive; field p; psi {psi}; gamma 1.4; fieldInf {reference_pressure}; lInf {l}; value uniform {reference_pressure};"
        )
    } else {
        far_pressure.clone()
    };
    write(
        dir,
        "0/p",
        header("volScalarField", "p")
            + &format!(
                "dimensions [{pressure_dims} 0 0 0 0]; internalField uniform {reference_pressure};\n"
            )
            + &boundary("type zeroGradient;", &far_pressure, &out_pressure),
    )?;
    if let Some(t) = cfg.air_temperature {
        let far_t = format!("type inletOutlet; inletValue uniform {t}; value uniform {t};");
        write(
            dir,
            "0/T",
            header("volScalarField", "T")
                + &format!("dimensions [0 0 0 1 0 0 0]; internalField uniform {t};\n")
                + &boundary("type zeroGradient;", &far_t, &far_t),
        )?;
        write(
            dir,
            "constant/thermophysicalProperties",
            header("dictionary", "thermophysicalProperties")
                + &format!(
                    "thermoType {{ type hePsiThermo; mixture pureMixture; transport const; thermo hConst; equationOfState perfectGas; specie specie; energy sensibleInternalEnergy; }}\nmixture {{ specie {{ molWeight 28.97; }} thermodynamics {{ Cp {}; Hf 0; }} transport {{ mu {}; Pr 0.72; }} }}",
                    3.5 * Config::GAS_CONSTANT,
                    cfg.density * cfg.kinematic_viscosity
                ),
        )?;
        let wall = if cfg.model == FlowModel::Laminar {
            "type calculated; value uniform 0;"
        } else {
            "type compressible::alphatWallFunction; value uniform 0;"
        };
        write(
            dir,
            "0/alphat",
            header("volScalarField", "alphat")
                + "dimensions [1 -1 -1 0 0 0 0]; internalField uniform 0;\n"
                + &boundary(
                    wall,
                    "type calculated; value uniform 0;",
                    "type calculated; value uniform 0;",
                ),
        )?;
    }
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
    let solver = cfg.solver();
    let force_density = if cfg.air_temperature.is_some() {
        "rho"
    } else {
        "rhoInf"
    };
    let energy_schemes = if cfg.air_temperature.is_some() {
        "div(phi,e) Gauss limitedLinear 1; div(phi,K) Gauss upwind; div(phi,Ekp) Gauss upwind; div(phid,p) Gauss upwind; div(phiv,p) Gauss upwind; div((phi|interpolate(rho)),p) Gauss upwind; div(((rho*nuEff)*dev2(T(grad(U))))) Gauss linear;"
    } else {
        ""
    };
    let end = if transient {
        cfg.transit_times * l / cfg.speed
    } else {
        cfg.iterations as f64
    };
    let convective_dt = l / cfg.speed;
    let near_wall_cell = if layers > 0 {
        first_height
    } else {
        surface_cell
    };
    let max_dt = if let Some(t) = cfg.air_temperature {
        (0.20 * near_wall_cell / (cfg.speed + Config::sound_speed(t))).min(convective_dt * 0.02)
    } else {
        convective_dt * 0.02
    };
    let dt = if transient {
        (convective_dt * 0.002).min(max_dt)
    } else {
        1.0
    };
    let advective_scheme = if cfg.air_temperature.is_some() && transient {
        "Gauss limitedLinearV 1"
    } else {
        "bounded Gauss linearUpwind grad(U)"
    };
    let reconstruction = if transient_gas {
        "reconstruct(rho) vanLeer; reconstruct(U) vanLeerV; reconstruct(T) vanLeer;"
    } else {
        ""
    };
    let conservative_equations = if transient_gas {
        "\"(rho|rhoU|rhoE)\" { solver diagonal; }"
    } else {
        ""
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
 loads {{ type forces; libs ("libforces.so"); patches (body); rho {force_density}; rhoInf {}; pRef {reference_pressure}; CofR {}; log true; writeControl timeStep; writeInterval 1; }}
 vort {{ type vorticity; libs ("libfieldFunctionObjects.so"); field U; result vorticity; executeControl writeTime; writeControl writeTime; }}
 yPlus {{ type yPlus; libs ("libfieldFunctionObjects.so"); executeControl writeTime; writeControl writeTime; }}
}}
"#,
                if transient { "true" } else { "false" },
                max_dt,
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
fluxScheme Kurganov;
ddtSchemes {{ default {}; }}
gradSchemes {{ default cellLimited Gauss linear 1; }}
divSchemes {{ default none; div(tauMC) Gauss linear; {energy_schemes} div(phi,U) {advective_scheme}; div(phi,k) bounded Gauss upwind; div(phi,omega) bounded Gauss upwind; div((nuEff*dev2(T(grad(U))))) Gauss linear; }}
laplacianSchemes {{ default Gauss linear limited 0.5; }}
interpolationSchemes {{ default linear; {reconstruction} }}
snGradSchemes {{ default limited 0.5; }}
wallDist {{ method meshWave; }}
"#,
                if transient { "Euler" } else { "steadyState" }
            ),
    )?;
    let pressure_controls = if cfg.air_temperature.is_some() {
        "consistent no; pMinFactor 0.1; pMaxFactor 2.0;"
    } else {
        "consistent yes;"
    };
    let relaxation = if cfg.air_temperature.is_some() && transient {
        "equations { \".*\" 1; }"
    } else if cfg.air_temperature.is_some() {
        "fields { p 0.45; rho 0.02; } equations { U 0.3; e 0.5; k 0.5; omega 0.5; }"
    } else {
        "fields { p 0.3; } equations { U 0.7; k 0.7; omega 0.7; }"
    };
    write(
        dir,
        "system/fvSolution",
        header("dictionary", "fvSolution")
            + &r#"
solvers {
 $CONSERVATIVE
 p { solver GAMG; tolerance 1e-8; relTol 0.05; smoother GaussSeidel; }
 pFinal { $p; relTol 0; }
 "(U|k|omega|e)" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0.1; }
 "(U|k|omega|e)Final" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0; }
}
SIMPLE { nNonOrthogonalCorrectors 1; $PRESSURE_CONTROL residualControl { p 1e-5; U 1e-6; "(k|omega|e)" 1e-6; } }
PIMPLE { nOuterCorrectors 2; nCorrectors 2; nNonOrthogonalCorrectors 1; }
relaxationFactors { $RELAXATION }
"#.replace("$CONSERVATIVE",conservative_equations).replace("$PRESSURE_CONTROL",pressure_controls).replace("$RELAXATION",relaxation),
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
}}
wallPressure {{ type surfaces; libs ("libsampling.so"); writeControl writeTime; surfaceFormat raw; fields (p); surfaces ( body {{ type patch; patches (body); interpolate false; }} ); }}
}}
"#
            ),
    )?;
    Ok(())
}
