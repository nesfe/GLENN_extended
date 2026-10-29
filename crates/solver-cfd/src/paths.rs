//! OpenFOAM runs `fileName::stripInvalid()` over the case path and over its own
//! working directory, so a directory such as "Продувка крыла" silently becomes
//! "Продувкакрыла" and the solver opens nothing. Relative paths do not help.
//!
//! Keep the user-facing directory exactly as chosen and hand the solver an
//! alias with an ASCII, space-free path: a symlink on Unix, a junction on
//! Windows. Only the alias is removed afterwards, never the real directory.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// The alias link and the private directory holding it.
struct Alias {
    root: PathBuf,
    link: PathBuf,
}

impl Drop for Alias {
    fn drop(&mut self) {
        // Remove the link itself. Never a recursive delete: the link points at
        // the user's results, and following it would destroy them.
        #[cfg(unix)]
        let _ = fs::remove_file(&self.link);
        #[cfg(windows)]
        let _ = fs::remove_dir(&self.link);
        let _ = fs::remove_dir(&self.root);
    }
}

pub(crate) struct SolverPath {
    pub path: PathBuf,
    _alias: Option<Alias>,
}

/// A path the solver can parse without losing characters.
fn usable(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.is_ascii() && !text.chars().any(char::is_whitespace)
}

/// Strip the extended-length prefix that `canonicalize` adds on Windows; the
/// solver's own path handling does not understand it.
fn plain(path: &Path) -> PathBuf {
    match path.to_str().and_then(|t| t.strip_prefix(r"\\?\")) {
        Some(stripped) => PathBuf::from(stripped),
        None => path.to_path_buf(),
    }
}

#[cfg(unix)]
fn alias_bases() -> Vec<PathBuf> {
    vec![PathBuf::from("/tmp"), std::env::temp_dir()]
}

#[cfg(windows)]
fn alias_bases() -> Vec<PathBuf> {
    // The user's own temp directory carries their account name, which may hold
    // spaces or Cyrillic, so machine-wide ASCII locations are kept as fallbacks.
    ["LOCALAPPDATA", "TEMP", "ProgramData", "SystemRoot"]
        .iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from))
        .map(|base| match base.file_name() {
            Some(name) if name.eq_ignore_ascii_case("Windows") => base.join("Temp"),
            _ if base.ends_with("Temp") => base,
            _ => base.join("Temp"),
        })
        .collect()
}

#[cfg(unix)]
fn link(target: &Path, alias: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, alias).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn link(target: &Path, alias: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // A directory junction needs no elevation, unlike a symbolic link, and
    // cmd's own quoting rules require the command line verbatim.
    let output = std::process::Command::new("cmd")
        .raw_arg(format!(
            "/D /C mklink /J \"{}\" \"{}\"",
            alias.display(),
            target.display()
        ))
        .output()
        .map_err(|e| format!("Не удалось создать ссылку для расчёта: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "mklink не создал ссылку для расчёта: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

impl SolverPath {
    pub fn new(path: &Path) -> Result<Self, String> {
        let direct = plain(path);
        if usable(&direct) {
            return Ok(Self {
                path: direct,
                _alias: None,
            });
        }
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let mut failure = None;
        for base in alias_bases().into_iter().filter(|b| usable(b)) {
            let root = base.join(format!(
                "glenn-cfd-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            if fs::create_dir_all(&root).is_err() {
                continue;
            }
            let alias = root.join("case");
            match link(&direct, &alias) {
                Ok(()) => {
                    return Ok(Self {
                        path: alias.clone(),
                        _alias: Some(Alias { root, link: alias }),
                    });
                }
                Err(e) => {
                    let _ = fs::remove_dir(&root);
                    failure = Some(e);
                }
            }
        }
        Err(failure.unwrap_or_else(|| format!(
            "Каталог расчёта {} содержит пробелы или не-ASCII символы, а создать служебную ссылку не удалось. Выберите каталог с латинскими буквами без пробелов: нативный OpenFOAM не разбирает такие пути.",
            direct.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_preserves_unicode_files_and_does_not_rename_the_user_directory() {
        let tmp = tempfile::Builder::new()
            .prefix("glenn-Продувка ")
            .tempdir()
            .unwrap();
        std::fs::write(tmp.path().join("value"), "geometry").unwrap();
        let alias = SolverPath::new(tmp.path()).unwrap();
        assert!(usable(&alias.path), "solver path {:?}", alias.path);
        assert_eq!(
            std::fs::read_to_string(alias.path.join("value")).unwrap(),
            "geometry"
        );
        drop(alias);
        // Dropping the alias must leave the user's own results untouched.
        assert!(tmp.path().join("value").exists());
    }

    #[test]
    fn an_already_usable_path_is_passed_through_unchanged() {
        let tmp = tempfile::Builder::new()
            .prefix("glenn-case-")
            .tempdir()
            .unwrap();
        let direct = SolverPath::new(tmp.path()).unwrap();
        assert_eq!(direct.path, plain(tmp.path()));
        assert!(direct._alias.is_none());
    }
}
