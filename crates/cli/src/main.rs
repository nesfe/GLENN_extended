//! Headless production path uses exactly the desktop's adapters and backends.
use glenn_solver_cfd::{Backend, Config, FlowModel, Resolution};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help") {
        println!(
            "glenn-cfd MODEL.stl --case NEW_DIRECTORY [--units m|mm|cm] [--mode laminar|rans|urans] [--quality preview|standard|fine] [--speed M/S] [--nu M2/S] [--density KG/M3] [--alpha DEG] [--iterations N] [--time L/V] [--bashrc PATH] [--wsl]"
        );
        return Ok(());
    }
    if args[0] == "--inspect" {
        let path = PathBuf::from(args.get(1).ok_or("Нужен каталог расчёта")?);
        let r = glenn_solver_cfd::load_result(&path)?;
        println!(
            "t={} force_N={:?} samples={} min_Ux={} converged={}",
            r.field.time,
            r.force.to_array(),
            r.field.velocity.iter().flatten().count(),
            r.field
                .velocity
                .iter()
                .flatten()
                .map(|v| v.x)
                .fold(f64::INFINITY, f64::min),
            r.converged
        );
        return Ok(());
    }
    let mut cfg = Config::default();
    let mut backend = Backend::default();
    let mut dir = None;
    let mut scale = 1.0;
    let mut iter = args[1..].iter();
    while let Some(flag) = iter.next() {
        if flag == "--wsl" {
            backend.wsl = true;
            continue;
        }
        let value = iter.next().ok_or_else(|| format!("Нет значения {flag}"))?;
        let number = || {
            value
                .parse::<f64>()
                .map_err(|_| format!("Неверное число: {value}"))
        };
        match flag.as_str() {
            "--case" => dir = Some(PathBuf::from(value)),
            "--bashrc" => backend.bashrc = value.clone(),
            "--units" => {
                scale = match value.as_str() {
                    "m" => 1.0,
                    "mm" => 0.001,
                    "cm" => 0.01,
                    _ => return Err("Единицы: m, mm, cm".into()),
                }
            }
            "--mode" => {
                cfg.model = match value.as_str() {
                    "laminar" => FlowModel::Laminar,
                    "rans" => FlowModel::SteadyRans,
                    "urans" => FlowModel::UnsteadyRans,
                    _ => return Err("Режим: laminar, rans, urans".into()),
                }
            }
            "--quality" => {
                cfg.resolution = match value.as_str() {
                    "preview" => Resolution::Preview,
                    "standard" => Resolution::Standard,
                    "fine" => Resolution::Fine,
                    _ => return Err("Сетка: preview, standard, fine".into()),
                }
            }
            "--speed" => cfg.speed = number()?,
            "--nu" => cfg.kinematic_viscosity = number()?,
            "--density" => cfg.density = number()?,
            "--alpha" => cfg.alpha_degrees = number()?,
            "--time" => cfg.transit_times = number()?,
            "--iterations" => {
                cfg.iterations = value.parse().map_err(|_| "Неверное число итераций")?
            }
            _ => return Err(format!("Неизвестный аргумент {flag}")),
        }
    }
    let dir = dir.ok_or("Требуется --case NEW_DIRECTORY")?;
    let mesh = glenn_io::stl::read(std::path::Path::new(&args[0]))?.scaled(scale)?;
    cfg.validate()?;
    let progress = Arc::new(Mutex::new(String::new()));
    let report = progress.clone();
    let worker = glenn_engine::Worker::new(move |(): (), cancel| {
        glenn_solver_cfd::run(&mesh, &cfg, &backend, &dir, cancel, &report)
    });
    worker.submit(());
    let mut previous = String::new();
    loop {
        let current = progress.lock().unwrap().clone();
        if previous != current {
            eprintln!("{current}");
            previous = current;
        }
        if let Some((_, result)) = worker.poll() {
            let r = result?;
            println!(
                "time={} force_N={:?} samples={} converged={} case={}",
                r.field.time,
                r.force.to_array(),
                r.field.velocity.iter().flatten().count(),
                r.converged,
                r.case_dir.display()
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
