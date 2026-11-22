//! Actualizaciones: busca en GitHub una release más nueva y la instala según cómo se instaló Slimcast.
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const API: &str = "https://api.github.com/repos/VuloB1/Slimcast/releases/latest";
// solo se descargan archivos de las releases de este repositorio
const DOWNLOADS: &str = "https://github.com/VuloB1/Slimcast/releases/download/";
const PAGES: &str = "https://github.com/VuloB1/Slimcast/releases/tag/v";
#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

/// Cómo se actualiza esta copia.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // cada sistema usa solo sus variantes
enum How {
    Setup,             // Windows: instalador silencioso
    AppImage(PathBuf), // Linux: se sustituye este archivo
    Page,              // Flatpak: no está en Flathub, así que se abre la página de la versión
}

#[derive(Clone)]
pub struct Release {
    pub version: String,
    url: String,
    how: How,
}

impl Release {
    /// Página de la versión en GitHub.
    pub fn web(&self) -> String {
        format!("{PAGES}{}", self.version)
    }

    /// La página, cuando la actualización es a mano (Flatpak).
    pub fn page(&self) -> Option<String> {
        (self.how == How::Page).then(|| self.web())
    }
}

// curl viene con Windows 10/11 y casi todas las distros; así no hace falta un cliente HTTP dentro del programa
fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-sfL", "-H", "User-Agent: Slimcast"]).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    c.creation_flags(NO_WINDOW);
    c
}

/// Cómo se instaló esta copia; None en desarrollo (sin instalador ni paquete), donde no se busca nada.
#[cfg(windows)]
fn installed() -> Option<How> {
    // junto a un Slimcast instalado está el desinstalador de Inno Setup
    std::env::current_exe().ok()?.with_file_name("unins000.exe").exists().then_some(How::Setup)
}

#[cfg(not(windows))]
fn installed() -> Option<How> {
    // el runtime de AppImage pone en APPIMAGE la ruta del archivo; Flatpak, FLATPAK_ID
    if let Some(path) = std::env::var_os("APPIMAGE") { return Some(How::AppImage(path.into())) }
    std::env::var_os("FLATPAK_ID").map(|_| How::Page)
}

/// Devuelve la release de GitHub si es más nueva que esta.
pub fn check() -> Option<Release> {
    let how = installed()?;
    let out = curl().args(["--max-time", "10", API]).output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout), env!("CARGO_PKG_VERSION"), how)
}

fn parse(json: &str, current: &str, how: How) -> Option<Release> {
    let version = values(json, "tag_name").next()?.trim_start_matches('v').to_string();
    let suffix = match how { How::Setup => "-setup.exe", How::AppImage(_) => "-x86_64.AppImage", How::Page => "-x86_64.flatpak" };
    let url = values(json, "browser_download_url").find(|u| u.starts_with(DOWNLOADS) && u.ends_with(suffix))?.to_string();
    (numbers(&version) > numbers(current)).then_some(Release { version, url, how })
}

// shortcut: lee cadenas JSON sin escapes, que es lo que usa GitHub en estos campos; usar serde_json si hiciera falta más
fn values<'a>(json: &'a str, key: &str) -> std::vec::IntoIter<&'a str> {
    let pat = format!("\"{key}\"");
    json.split(pat.as_str()).skip(1).filter_map(|rest| {
        let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
        Some(&rest[..rest.find('"')?])
    }).collect::<Vec<_>>().into_iter()
}

fn numbers(v: &str) -> Vec<u32> {
    v.split('.').map(|n| n.parse().unwrap_or(0)).collect()
}

fn download(url: &str, to: &Path) -> bool {
    curl().args(["--max-time", "900", "-o"]).arg(to).arg(url).status().is_ok_and(|s| s.success())
}

/// Descarga e instala la versión nueva y vuelve a abrir Slimcast; tras Ok la app debe cerrarse.
/// Err(None) significa que no se puede hacer aquí y hay que mandar al usuario a la página de la versión.
pub fn install(r: &Release) -> Result<(), Option<String>> {
    match &r.how {
        How::Setup => {
            let setup: PathBuf = std::env::temp_dir().join(format!("Slimcast-{}-setup.exe", r.version));
            if !download(&r.url, &setup) { return Err(Some(crate::i18n::t("download_failed").into())) }
            Command::new(&setup).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]).spawn()
                .map(|_| ()).map_err(|_| Some(crate::i18n::t("setup_failed").into()))
        }
        How::AppImage(path) => {
            // shortcut: sin permiso para escribir junto al AppImage (p. ej. en /opt) se manda a la página, no se pide sudo
            replace(path, |tmp| download(&r.url, tmp))?;
            Command::new(path).spawn().map(|_| ()).map_err(|_| Some(crate::i18n::t("setup_failed").into()))
        }
        How::Page => Err(None),
    }
}

/// Sustituye `target` por lo que `fetch` deje en un archivo temporal a su lado (mismo disco: el cambio es
/// atómico y nunca queda un AppImage a medias). Linux permite reemplazar el archivo mientras se ejecuta.
fn replace(target: &Path, fetch: impl FnOnce(&Path) -> bool) -> Result<(), Option<String>> {
    let name = target.file_name().ok_or(None)?.to_string_lossy();
    let tmp = target.with_file_name(format!(".{name}.update"));
    let result = (|| {
        if std::fs::File::create(&tmp).is_err() { return Err(None) } // carpeta sin permiso de escritura
        if !fetch(&tmp) { return Err(Some(crate::i18n::t("download_failed").to_string())) }
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|_| None)?;
        std::fs::rename(&tmp, target).map_err(|_| None)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&tmp); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"tag_name": "v1.10.0", "assets": [
        {"browser_download_url": "https://evil.example/Slimcast-1.10.0-setup.exe"},
        {"browser_download_url":"https://github.com/VuloB1/Slimcast/releases/download/v1.10.0/Slimcast-1.10.0-setup.exe"},
        {"browser_download_url":"https://github.com/VuloB1/Slimcast/releases/download/v1.10.0/Slimcast-1.10.0-x86_64.AppImage"},
        {"browser_download_url":"https://github.com/VuloB1/Slimcast/releases/download/v1.10.0/Slimcast-1.10.0-x86_64.flatpak"}]}"#;

    #[test]
    fn newer_release_found() {
        let r = parse(JSON, "1.9.3", How::Setup).unwrap();
        assert_eq!(r.version, "1.10.0");
        assert!(r.url.starts_with(DOWNLOADS) && r.url.ends_with("-setup.exe")); // ignora enlaces fuera del repositorio
        assert!(r.page().is_none());
    }

    #[test]
    fn same_or_older_ignored() {
        assert!(parse(JSON, "1.10.0", How::Setup).is_none());
        assert!(parse(JSON, "2.0.0", How::Setup).is_none());
    }

    #[test]
    fn each_install_kind_picks_its_file() {
        let a = parse(JSON, "1.9.3", How::AppImage("/x/Slimcast.AppImage".into())).unwrap();
        assert!(a.url.ends_with("-x86_64.AppImage"));
        let f = parse(JSON, "1.9.3", How::Page).unwrap();
        assert_eq!(f.page().as_deref(), Some("https://github.com/VuloB1/Slimcast/releases/tag/v1.10.0"));
        // una release sin AppImage (las antiguas) no ofrece actualizar a quien usa el AppImage
        let only_exe = r#"{"tag_name":"v1.10.0","assets":[{"browser_download_url":"https://github.com/VuloB1/Slimcast/releases/download/v1.10.0/Slimcast-1.10.0-setup.exe"}]}"#;
        assert!(parse(only_exe, "1.9.3", How::AppImage("/x/a".into())).is_none());
    }

    #[test]
    fn replace_swaps_the_file_or_leaves_it_untouched() {
        let dir = std::env::temp_dir().join(format!("slimcast-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Slimcast.AppImage");
        std::fs::write(&target, "viejo").unwrap();
        // descarga fallida: el AppImage sigue igual y no queda el temporal
        assert!(replace(&target, |_| false).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "viejo");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // descarga buena: se sustituye y queda ejecutable
        assert!(replace(&target, |tmp| std::fs::write(tmp, "nuevo").is_ok()).is_ok());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "nuevo");
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&target).unwrap().permissions()) & 0o777, 0o755);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
