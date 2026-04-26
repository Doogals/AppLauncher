use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub path: String,
}

pub fn get_installed_apps() -> Vec<InstalledApp> {
    let should_uninit = unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok()
    };

    let mut lnk_files = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        collect_lnk_files(&path, &mut lnk_files);
    }

    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        let path = PathBuf::from(programdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        collect_lnk_files(&path, &mut lnk_files);
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut apps: Vec<InstalledApp> = lnk_files
        .iter()
        .filter_map(|lnk| {
            let name = lnk.file_stem()?.to_string_lossy().into_owned();
            let target = resolve_lnk(lnk)?;
            if seen.insert(target.to_ascii_lowercase()) {
                Some(InstalledApp { name, path: target })
            } else {
                None
            }
        })
        .collect();

    apps.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    if should_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }

    apps
}

fn collect_lnk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
            out.push(path);
        }
    }
}

fn resolve_lnk(lnk_path: &Path) -> Option<String> {
    use windows::{
        core::{Interface, PCWSTR},
        Win32::Storage::FileSystem::WIN32_FIND_DATAW,
        Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, IPersistFile, STGM_READ},
        Win32::UI::Shell::{IShellLinkW, ShellLink},
    };

    unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let wide: Vec<u16> = lnk_path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        persist_file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut buf = [0u16; 1024]; // INFOTIPSIZE — Shell-recommended buffer for GetPath
        let mut find_data = WIN32_FIND_DATAW::default();
        shell_link
            .GetPath(&mut buf, &mut find_data, 0)
            .ok()?;

        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let target = String::from_utf16_lossy(&buf[..end]);

        if target.is_empty() || !target.to_ascii_lowercase().ends_with(".exe") {
            return None;
        }
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn collect_lnk_files_finds_lnk_and_ignores_others() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.lnk"), b"").unwrap();
        fs::write(dir.join("readme.txt"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "app.lnk");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_lnk_files_recurses_into_subdirs() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_recurse_test");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.lnk"), b"").unwrap();
        fs::write(sub.join("b.lnk"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);
        assert_eq!(found.len(), 2);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn get_installed_apps_returns_vec_without_panic() {
        let apps = get_installed_apps();
        // Do not assert non-empty — a clean CI environment may have no .lnk shortcuts.
        for app in &apps {
            assert!(
                app.path.to_ascii_lowercase().ends_with(".exe"),
                "Non-exe path: {}",
                app.path
            );
        }
    }
}
