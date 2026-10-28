//! OpenFOAM fileName strips whitespace. Keep the user-facing directory intact
//! and pass a filesystem alias to the native solver, never a lossy string edit.
use std::path::{Path, PathBuf};
pub(crate) struct SolverPath {
    pub path: PathBuf,
    #[cfg(unix)]
    _directory: Option<tempfile::TempDir>,
}
impl SolverPath {
    pub fn new(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        {
            use normpath::PathExt;
            let short = path
                .shorten()
                .map_err(|e| format!("Путь решателя: {e}"))?
                .into_owned();
            let text = short.to_string_lossy();
            let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
            if !text.is_ascii() || text.chars().any(char::is_whitespace) {
                return Err("Файловая система не предоставила короткий путь для CFD. Выберите каталог с латинскими буквами без пробелов на локальном NTFS-диске; это ограничение нативного OpenFOAM.".into());
            }
            Ok(Self {
                path: PathBuf::from(text),
            })
        }
        #[cfg(unix)]
        {
            if path.to_string_lossy().chars().any(char::is_whitespace) {
                let directory = tempfile::Builder::new()
                    .prefix("glenn-cfd-")
                    .tempdir_in("/tmp")
                    .map_err(|e| e.to_string())?;
                let alias = directory.path().join("data");
                std::os::unix::fs::symlink(path, &alias).map_err(|e| e.to_string())?;
                Ok(Self {
                    path: alias,
                    _directory: Some(directory),
                })
            } else {
                Ok(Self {
                    path: path.to_path_buf(),
                    _directory: None,
                })
            }
        }
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
        assert!(
            !alias
                .path
                .to_string_lossy()
                .chars()
                .any(char::is_whitespace)
        );
        assert_eq!(
            std::fs::read_to_string(alias.path.join("value")).unwrap(),
            "geometry"
        );
        drop(alias);
        assert!(tmp.path().join("value").exists());
    }
}
