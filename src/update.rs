//! Actualizaciones: busca en GitHub una release más nueva y la instala con su propio instalador.
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const API: &str = "https://api.github.com/repos/VuloB1/Slimcast/releases/latest";
// solo se descargan instaladores de las releases de este repositorio
const DOWNLOADS: &str = "https://github.com/VuloB1/Slimcast/releases/download/";
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone)]
pub struct Release {
    pub version: String,
    url: String,
}

// curl viene con Windows 10/11; así no hace falta un cliente HTTP dentro del programa
fn curl() -> Command {
    let mut c = Command::new("curl.exe");
    c.args(["-sfL", "-H", "User-Agent: Slimcast"]).creation_flags(NO_WINDOW);
    c
}

/// Devuelve la release de GitHub si es más nueva que esta. Solo en copias instaladas
/// (junto al .exe está el desinstalador); en desarrollo no hace nada.
pub fn check() -> Option<Release> {
    let exe = std::env::current_exe().ok()?;
    if !exe.with_file_name("unins000.exe").exists() { return None }
    let out = curl().args(["--max-time", "10", API]).output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout), env!("CARGO_PKG_VERSION"))
}

fn parse(json: &str, current: &str) -> Option<Release> {
    let version = values(json, "tag_name").next()?.trim_start_matches('v').to_string();
    let url = values(json, "browser_download_url")
        .find(|u| u.starts_with(DOWNLOADS) && u.ends_with("-setup.exe"))?.to_string();
    (numbers(&version) > numbers(current)).then_some(Release { version, url })
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

/// Descarga el instalador y lo lanza en modo silencioso; al terminar vuelve a abrir Slimcast.
pub fn install(r: &Release) -> Result<(), String> {
    let setup: PathBuf = std::env::temp_dir().join(format!("Slimcast-{}-setup.exe", r.version));
    let ok = curl().args(["--max-time", "900", "-o"]).arg(&setup).arg(&r.url).status().is_ok_and(|s| s.success());
    if !ok { return Err(crate::i18n::t("download_failed").into()) }
    Command::new(&setup).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]).spawn()
        .map(|_| ()).map_err(|_| crate::i18n::t("setup_failed").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"tag_name": "v1.10.0", "assets": [
        {"browser_download_url": "https://evil.example/Slimcast-1.10.0-setup.exe"},
        {"browser_download_url":"https://github.com/VuloB1/Slimcast/releases/download/v1.10.0/Slimcast-1.10.0-setup.exe"}]}"#;

    #[test]
    fn newer_release_found() {
        let r = parse(JSON, "1.9.3").unwrap();
        assert_eq!(r.version, "1.10.0");
        assert!(r.url.starts_with(DOWNLOADS)); // ignora enlaces fuera del repositorio
    }

    #[test]
    fn same_or_older_ignored() {
        assert!(parse(JSON, "1.10.0").is_none());
        assert!(parse(JSON, "2.0.0").is_none());
    }
}
