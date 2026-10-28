use crate::{Config, FlowModel, VolumeField, read_field, write_case};
use glam::DVec3;
use glenn_core::mesh::SolidMesh;
use glenn_engine::Cancellation;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default)]
pub struct Backend {
    /// Expert override. Empty selects the runtime bundled beside the executable.
    pub bashrc: String,
    /// Windows uses an installed WSL distribution; all paths passed as argv.
    pub wsl: bool,
}
pub struct RunResult {
    pub field: VolumeField,
    pub force: DVec3,
    pub case_dir: PathBuf,
    pub converged: bool,
    pub streamlines: Vec<Vec<DVec3>>,
}
fn posix_path(path: &Path, wsl: bool) -> Result<String, String> {
    let raw = path.to_str().ok_or("Путь должен быть в Unicode")?;
    let text = raw.strip_prefix(r"\\?\").unwrap_or(raw);
    if wsl {
        let bytes = text.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            return Ok(format!(
                "/mnt/{}/{}",
                (bytes[0] as char).to_ascii_lowercase(),
                text[3..].replace('\\', "/")
            ));
        }
        return Err(
            "Для WSL выберите каталог на локальном диске, например C:\\GLENN\\cases".into(),
        );
    }
    Ok(text.into())
}
impl Backend {
    fn command(
        &self,
        tool: &str,
        case: &Path,
        args: &[&str],
        runtime: Option<&crate::runtime::Runtime>,
    ) -> Result<Command, String> {
        let case_path = posix_path(case, self.wsl)?;
        // No interpolated shell text. The script is constant; every path and
        // argument is passed positionally, including spaces and shell characters.
        let script = r#"glenn_source() { local init="$1"; shift; source "$init"; }
if [ -n "$1" ]; then glenn_source "$1" || exit; fi
shift
cancel="$1"; shift
"$@" & pid=$!
trap 'kill "$pid" 2>/dev/null; wait "$pid"; exit 130' TERM INT
while kill -0 "$pid" 2>/dev/null; do
 if [ -f "$cancel" ]; then kill "$pid" 2>/dev/null; wait "$pid"; exit 130; fi
 sleep 0.2
done
wait "$pid"
"#;
        let mut cmd = if self.wsl {
            let mut c = Command::new("wsl.exe");
            c.args([
                "--exec",
                "bash",
                "--noprofile",
                "--norc",
                "-c",
                script,
                "glenn",
            ]);
            c
        } else if !self.bashrc.is_empty() {
            let mut c = Command::new("bash");
            c.args(["--noprofile", "--norc", "-c", script, "glenn"]);
            c
        } else {
            let mut c = runtime
                .ok_or("Комплектный решатель не подготовлен")?
                .command(tool)?;
            c.arg("-case").arg(&case_path);
            c.args(args);
            return Ok(c);
        };
        cmd.arg(&self.bashrc)
            .arg(format!("{case_path}/.cancel"))
            .arg(tool)
            .arg("-case")
            .arg(case_path)
            .args(args);
        Ok(cmd)
    }
}
// WSL emits UTF-16 on Windows, whereas solver logs are UTF-8. Never silently
// discard the diagnostic because read_to_string rejected its encoding.
fn decode_log(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xff, 0xfe])
        || (bytes.len() >= 8
            && bytes.chunks_exact(2).take(64).filter(|c| c[1] == 0).count()
                > bytes.len().min(128) / 8);
    if utf16 {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
#[cfg(test)]
mod log_tests {
    use super::decode_log;
    #[test]
    fn windows_and_native_diagnostics_remain_readable() {
        let message = "WSL: нет установленных дистрибутивов. wsl.exe --install";
        let encoded: Vec<_> = message.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_log(&encoded), message);
        assert_eq!(decode_log(message.as_bytes()), message);
    }
}
fn stage(
    backend: &Backend,
    case: &Path,
    tool: &str,
    args: &[&str],
    token: &Cancellation,
    progress: &Arc<Mutex<String>>,
) -> Result<(), String> {
    token.check()?;
    *progress.lock().unwrap() = format!("{tool}: {}", case.display());
    let log_name = Path::new(tool)
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or("solver");
    let log_path = case.join(format!("log.{log_name}"));
    let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let runtime = if backend.bashrc.is_empty() && !backend.wsl {
        Some(crate::runtime::Runtime::discover()?)
    } else {
        None
    };
    let mut child=backend.command(tool,case,args,runtime.as_ref())?.stdout(Stdio::from(log)).stderr(Stdio::from(err)).spawn()
        .map_err(|e|format!("Не удалось запустить {tool}: {e}. Проверьте целостность полного архива GLENN и журнал запуска."))?;
    let mut reported = Instant::now();
    loop {
        if token.is_cancelled() {
            fs::write(case.join(".cancel"), b"cancel").map_err(|e| e.to_string())?;
            if !backend.wsl && backend.bashrc.is_empty() {
                let _ = child.kill();
            }
            let _ = child.wait();
            return Err("Расчёт отменён; журнал сохранён".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                break;
            }
            let log = decode_log(&fs::read(&log_path).unwrap_or_default());
            let tail = log
                .lines()
                .rev()
                .take(18)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!(
                "{tool} завершился с ошибкой. Журнал: {}\n{tail}",
                log_path.display()
            ));
        }
        if reported.elapsed() >= Duration::from_millis(500) {
            // Read a bounded tail, not an ever-growing solver log on every frame.
            if let Ok(mut file) = fs::File::open(&log_path) {
                let size = file.metadata().map_or(0, |m| m.len());
                let _ = file.seek(SeekFrom::Start(size.saturating_sub(8192)));
                let mut tail = String::new();
                if file.read_to_string(&mut tail).is_ok()
                    && let Some(time) = tail
                        .lines()
                        .rev()
                        .find_map(|line| line.strip_prefix("Time = "))
                {
                    *progress.lock().unwrap() =
                        format!("{tool}: t / итерация {time} · {}", case.display());
                }
            }
            reported = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
pub fn run(
    mesh: &SolidMesh,
    cfg: &Config,
    backend: &Backend,
    case: &Path,
    token: &Cancellation,
    progress: &Arc<Mutex<String>>,
) -> Result<RunResult, String> {
    token.check()?;
    if backend.bashrc.is_empty() && !backend.wsl {
        crate::runtime::Runtime::discover()?;
    }
    write_case(case, mesh, cfg)?;
    let original_case = case.canonicalize().map_err(|e| e.to_string())?;
    let alias = if !backend.wsl {
        Some(crate::paths::SolverPath::new(&original_case)?)
    } else {
        None
    };
    let case = alias.as_ref().map_or(&original_case, |a| &a.path);
    stage(backend, case, "blockMesh", &[], token, progress)?;
    stage(
        backend,
        case,
        "snappyHexMesh",
        &["-overwrite"],
        token,
        progress,
    )?;
    stage(backend, case, "checkMesh", &[], token, progress)?;
    let mesh_log = fs::read_to_string(case.join("log.checkMesh")).map_err(|e| e.to_string())?;
    if !mesh_log.contains("Mesh OK") {
        return Err(format!(
            "Сетка не прошла checkMesh. Проверьте {}",
            case.join("log.checkMesh").display()
        ));
    }
    let solver = if cfg.model == FlowModel::UnsteadyRans {
        "pimpleFoam"
    } else {
        "simpleFoam"
    };
    stage(backend, case, solver, &[], token, progress)?;
    stage(
        backend,
        case,
        "postProcess",
        &[
            "-dict",
            "system/sampleDict",
            "-latestTime",
            "-fields",
            "(U p vorticity)",
        ],
        token,
        progress,
    )?;
    token.check()?;
    fs::write(case.join("result.foam"), b"").map_err(|e| e.to_string())?;
    load_result(&original_case).inspect(|result| {
        *progress.lock().unwrap() = if cfg.model == FlowModel::UnsteadyRans {
            "Нестационарный расчёт завершён; показан последний момент".into()
        } else if result.converged {
            "Сходимость по заданным невязкам достигнута".into()
        } else {
            "Достигнут лимит итераций; сходимость не подтверждена".into()
        };
    })
}

/// Reopen a completed case without rerunning or mixing it with current inputs.
pub fn load_result(case: &Path) -> Result<RunResult, String> {
    let cfg: Config = serde_json::from_str(
        &fs::read_to_string(case.join("glenn-case.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    cfg.validate()?;
    let solver = if cfg.model == FlowModel::UnsteadyRans {
        "pimpleFoam"
    } else {
        "simpleFoam"
    };
    let field = read_field(case, cfg.density)?;
    let (force_time, force) = crate::field::loads(case)?;
    if (force_time - field.time).abs() > 1e-5 * field.time.abs().max(1.0) {
        return Err("Силы и поле относятся к разным моментам; результат не опубликован".into());
    }
    let log = fs::read_to_string(case.join(format!("log.{solver}"))).map_err(|e| e.to_string())?;
    let converged = log.contains("SIMPLE solution converged");
    let streamlines = field.streamlines();
    Ok(RunResult {
        field,
        force,
        case_dir: case.to_path_buf(),
        converged,
        streamlines,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use glenn_engine::Worker;
    use std::{os::unix::fs::PermissionsExt, sync::mpsc, time::Instant};
    #[test]
    fn cancellation_stops_the_external_process_even_with_a_bashrc() {
        let root = std::env::temp_dir().join(format!(
            "glenn-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let tool = root.join("test-solver");
        fs::write(&tool, "#!/bin/sh\necho $$ > \"$2/pid\"\nexec sleep 60\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let bashrc = root.join("init with spaces.sh");
        fs::write(&bashrc, "# test environment\n").unwrap();
        let backend = Backend {
            bashrc: bashrc.to_string_lossy().into(),
            wsl: false,
        };
        let case = root.clone();
        let (done, receive) = mpsc::channel();
        let worker = Worker::new(move |(): (), token| {
            let result = stage(
                &backend,
                &case,
                tool.to_str().unwrap(),
                &[],
                token,
                &Arc::new(Mutex::new(String::new())),
            );
            done.send(result.clone()).unwrap();
            result
        });
        worker.submit(());
        let until = Instant::now() + Duration::from_secs(10);
        while !root.join("pid").exists() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = fs::read_to_string(root.join("pid")).unwrap();
        worker.cancel();
        assert!(
            receive
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err()
                .contains("отменён")
        );
        assert!(
            !Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
