//! Relocatable runtime shipped beside both desktop and CLI executables.
//! No installer, shell activation, registry or machine-wide PATH is required.
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub const TOOLS: [&str; 6] = [
    "blockMesh",
    "snappyHexMesh",
    "checkMesh",
    "simpleFoam",
    "pimpleFoam",
    "postProcess",
];
#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    version: String,
    platform: String,
    bin: PathBuf,
    libraries: Vec<PathBuf>,
    /// Values are runtime-relative directories, never shell expressions.
    environment: BTreeMap<String, PathBuf>,
}
pub(crate) struct Runtime {
    root: PathBuf,
    manifest: Manifest,
    _alias: crate::paths::SolverPath,
}
impl Runtime {
    pub fn discover() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        Self::load(
            &exe.parent()
                .ok_or("Не найден каталог приложения")?
                .join("runtime/openfoam"),
        )
    }
    fn load(root: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(root.join("runtime.json")).map_err(|_| format!(
            "В комплекте приложения отсутствует расчётный модуль: {}. Распакуйте полный архив GLENN, сохранив папку runtime рядом с приложением. Отдельный EXE не является полной поставкой.", root.display()))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| format!("Повреждён манифест решателя: {e}"))?;
        if manifest.schema != 1
            || manifest.version != "2412"
            || manifest.platform != format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
        {
            return Err(
                "Комплектный решатель несовместим с этой версией или платформой GLENN".into(),
            );
        }
        for path in std::iter::once(&manifest.bin)
            .chain(&manifest.libraries)
            .chain(manifest.environment.values())
        {
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
                || !root.join(path).is_dir()
            {
                return Err(format!(
                    "Повреждён каталог комплектного решателя: {}",
                    path.display()
                ));
            }
        }
        for tool in TOOLS {
            if !root
                .join(&manifest.bin)
                .join(format!("{tool}{}", std::env::consts::EXE_SUFFIX))
                .is_file()
            {
                return Err(format!(
                    "В комплектном решателе отсутствует {tool}; восстановите полный архив GLENN"
                ));
            }
        }
        let alias = crate::paths::SolverPath::new(root)?;
        Ok(Self {
            root: alias.path.clone(),
            manifest,
            _alias: alias,
        })
    }
    pub fn command(&self, tool: &str) -> Result<Command, String> {
        if !TOOLS.contains(&tool) {
            return Err(format!("Неизвестная команда решателя: {tool}"));
        }
        let bin = self.root.join(&self.manifest.bin);
        let mut cmd = Command::new(bin.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)));
        for (key, relative) in &self.manifest.environment {
            cmd.env(key, self.root.join(relative));
        }
        cmd.env("WM_PROJECT_VERSION", "v2412");
        cmd.env("FOAM_API", "2412");
        let libraries: Vec<_> = self
            .manifest
            .libraries
            .iter()
            .map(|p| self.root.join(p))
            .collect();
        let mut path = vec![bin];
        path.extend(libraries.iter().cloned());
        if let Some(existing) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&existing));
        }
        cmd.env(
            "PATH",
            std::env::join_paths(path).map_err(|e| e.to_string())?,
        );
        let key = if cfg!(target_os = "macos") {
            "DYLD_LIBRARY_PATH"
        } else {
            "LD_LIBRARY_PATH"
        };
        cmd.env(
            key,
            std::env::join_paths(libraries).map_err(|e| e.to_string())?,
        );
        // OpenFOAM must not pick user/site dictionaries from a different release.
        cmd.env("FOAM_CONFIG_MODE", "o");
        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_complete_runtime_and_relocates_without_path_lookup() {
        let root = std::env::temp_dir().join(format!("glenn runtime {}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("etc")).unwrap();
        for tool in TOOLS {
            fs::write(
                root.join("bin")
                    .join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
                "",
            )
            .unwrap();
        }
        let mut manifest = serde_json::json!({"schema":1,"version":"2412","platform":format!("{}-{}", std::env::consts::OS,std::env::consts::ARCH),"bin":"bin","libraries":["lib"],"environment":{"FOAM_ETC":"etc"}});
        fs::write(root.join("runtime.json"), manifest.to_string()).unwrap();
        let runtime = Runtime::load(&root).unwrap();
        let command = runtime.command("simpleFoam").unwrap();
        assert_eq!(
            Path::new(command.get_program()).parent().unwrap(),
            runtime.root.join("bin")
        );
        assert!(runtime.command("../../other").is_err());
        manifest["libraries"] = serde_json::json!(["../outside"]);
        fs::write(root.join("runtime.json"), manifest.to_string()).unwrap();
        assert!(Runtime::load(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
        assert!(Runtime::load(&root).is_err());
    }
}
