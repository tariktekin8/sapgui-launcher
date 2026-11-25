use crate::systems::{Logon, System};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Connection string of SAP GUI for Java. `&` separates its parameters, so a value with `&` could add or cut one:
/// a cut password would be a failed logon. Such a value is an error.
pub fn conn_string(s: &System, l: &Logon) -> Result<String, String> {
    let fields = [
        ("host", &s.host),
        ("system number", &s.sysnr),
        ("client", &l.client),
        ("user", &l.user),
        ("language", &l.language),
        ("password", &l.password),
    ];
    if let Some((name, _)) = fields.iter().find(|(_, value)| value.contains('&')) {
        return Err(format!("The {name} contains \"&\", which cannot be passed to SAP GUI on this system. Change it, or log on by hand."));
    }
    let mut conn = format!("conn=/H/{}/S/32{}", s.host, s.sysnr);
    // TODO: test passwords that contain '='.
    for (key, value) in [("clnt", &l.client), ("user", &l.user), ("lang", &l.language), ("pass", &l.password)] {
        if !value.is_empty() {
            conn.push_str(&format!("&{key}={value}"));
        }
    }
    Ok(conn)
}

pub fn args(s: &System, l: &Logon) -> Result<Vec<String>, String> {
    if cfg!(windows) {
        Ok(vec![
            format!("-system={}", s.sid),
            format!("-client={}", l.client),
            format!("-user={}", l.user),
            format!("-pw={}", l.password),
            format!("-language={}", l.language),
        ])
    } else {
        Ok(vec![conn_string(s, l)?])
    }
}

pub fn launch(sapgui: &str, s: &System, l: &Logon) -> Result<(), String> {
    if s.host.is_empty() && !cfg!(windows) {
        return Err(format!("Add a host to \"{}\" before you launch it.", s.name));
    }
    if sapgui.is_empty() {
        return Err("SAP GUI was not found. Choose it under Settings.".into());
    }
    // Args are passed directly, never through a shell.
    Command::new(sapgui)
        .args(args(s, l)?)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start SAP GUI at \"{sapgui}\" ({e}). Choose it under Settings."))
}

const EXE: &str = if cfg!(windows) {
    "sapshcut.exe"
} else if cfg!(target_os = "macos") {
    "Contents/MacOS/SAPGUI"
} else {
    // shortcut: Linux path not verified on a real install yet.
    "bin/sapgui"
};

// 64-bit SAP GUI (8.0+) first, then the older 32-bit install path.
#[cfg(windows)]
const ROOTS: &[&str] = &[r"C:\Program Files\SAP\FrontEnd", r"C:\Program Files (x86)\SAP\FrontEnd"];
#[cfg(target_os = "macos")]
const ROOTS: &[&str] = &["/Applications/SAP Clients"];
#[cfg(not(any(windows, target_os = "macos")))]
const ROOTS: &[&str] = &["/opt/SAPClients"];

/// What the user chooses in the file picker.
pub const CHOOSE_TITLE: &str = if cfg!(windows) {
    "Choose sapshcut.exe"
} else if cfg!(target_os = "macos") {
    "Choose the SAPGUI app"
} else {
    "Choose the sapgui file"
};

/// The SAP GUI executable for the file the user chose: sapshcut.exe, the SAPGUI app (macOS) or sapgui (Linux).
pub fn sapgui_from_file(file: &Path) -> Option<PathBuf> {
    let exe = if cfg!(target_os = "macos") && file.extension().is_some_and(|e| e == "app") {
        file.join(EXE)
    } else {
        file.to_path_buf()
    };
    let name = Path::new(EXE).file_name()?.to_string_lossy().into_owned();
    let matches = exe.file_name()?.to_string_lossy().eq_ignore_ascii_case(&name);
    (matches && exe.is_file()).then_some(exe)
}

/// The SAP GUI executable in a folder, or in its "SAPGUI*" folders, newest first.
fn find_sapgui(dir: &Path) -> Option<PathBuf> {
    // deepest on macOS: SAP Clients / SAPGUI <version> / SAPGUI <version>.app
    find_below(dir, 2)
}

fn find_below(dir: &Path, depth: usize) -> Option<PathBuf> {
    let exe = dir.join(EXE);
    if exe.is_file() {
        return Some(exe);
    }
    if depth == 0 {
        return None;
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with("sapgui"))
        .map(|e| e.path())
        .collect();
    folders.sort();
    folders.iter().rev().find_map(|f| find_below(f, depth - 1))
}

pub fn default_sapgui_path() -> String {
    ROOTS
        .iter()
        .find_map(|root| find_sapgui(Path::new(root)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Where the file picker opens: next to the current SAP GUI, or in the first SAP folder of this computer.
pub fn picker_folder(current: &str) -> Option<PathBuf> {
    // macOS: <folder>/SAPGUI.app/Contents/MacOS/SAPGUI
    let up = if cfg!(target_os = "macos") { 4 } else { 1 };
    Path::new(current)
        .ancestors()
        .nth(up)
        .filter(|p| p.is_absolute() && p.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| ROOTS.iter().map(PathBuf::from).find(|p| p.is_dir()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> System {
        System {
            name: "DEV".into(),
            sid: "DEV".into(),
            host: "dev.example".into(),
            sysnr: "00".into(),
            ..Default::default()
        }
    }

    fn logon() -> Logon {
        Logon {
            client: "100".into(),
            user: "DEMO".into(),
            language: "EN".into(),
            password: "secret".into(),
            ..Default::default()
        }
    }

    #[test]
    fn conn_string_full_and_without_empty_fields() {
        assert_eq!(
            conn_string(&sys(), &logon()).unwrap(),
            "conn=/H/dev.example/S/3200&clnt=100&user=DEMO&lang=EN&pass=secret"
        );
        let l = Logon { password: String::new(), language: String::new(), ..logon() };
        assert_eq!(conn_string(&sys(), &l).unwrap(), "conn=/H/dev.example/S/3200&clnt=100&user=DEMO");
    }

    #[test]
    fn args_per_os() {
        let a = args(&sys(), &logon()).unwrap();
        if cfg!(windows) {
            assert_eq!(a[0], "-system=DEV");
            assert_eq!(a[3], "-pw=secret");
        } else {
            assert_eq!(a, vec![conn_string(&sys(), &logon()).unwrap()]);
        }
    }

    #[test]
    fn a_value_cannot_add_connection_parameters() {
        let err = conn_string(&sys(), &Logon { password: "top&secret".into(), ..logon() }).unwrap_err();
        assert!(err.contains("password") && !err.contains("top"));
        assert!(conn_string(&sys(), &Logon { user: "A&tran=SU01".into(), ..logon() }).is_err());
        assert!(conn_string(&System { host: "h/S/3200&tran=SU01".into(), ..sys() }, &logon()).is_err());
        assert!(conn_string(&sys(), &Logon { password: "a=b".into(), ..logon() }).is_ok());
    }

    #[test]
    fn finds_the_newest_sapgui_below_a_folder() {
        let root = std::env::temp_dir().join(format!("sapgui-launcher-find-{}", std::process::id()));
        assert_eq!(find_sapgui(&root), None);
        for version in ["SAPGUI 8.00", "SAPGUI 8.10"] {
            let exe = root.join(version).join(EXE);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(exe, "").unwrap();
        }
        std::fs::create_dir_all(root.join("SAPGUI 9 empty")).unwrap();
        let newest = root.join("SAPGUI 8.10").join(EXE);
        assert_eq!(find_sapgui(&root), Some(newest.clone()));
        assert_eq!(find_sapgui(&root.join("SAPGUI 8.10")), Some(newest));
        assert_eq!(find_sapgui(&root.join("SAPGUI 9 empty")), None);

        let chosen = root.join("SAPGUI 8.10").join(EXE);
        assert_eq!(sapgui_from_file(&chosen), Some(chosen.clone()));
        assert_eq!(sapgui_from_file(&root.join("SAPGUI 8.10")), None);
        assert_eq!(sapgui_from_file(&root.join("missing").join(EXE)), None);
        let other = root.join("other.exe");
        std::fs::write(&other, "").unwrap();
        assert_eq!(sapgui_from_file(&other), None);
        if cfg!(target_os = "macos") {
            let app = root.join("SAPGUI 8.20.app");
            std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            assert_eq!(sapgui_from_file(&app), None);
            std::fs::write(app.join(EXE), "").unwrap();
            assert_eq!(sapgui_from_file(&app), Some(app.join(EXE)));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn launch_reports_missing_executable() {
        let err = launch("/nonexistent/sapgui", &sys(), &logon()).unwrap_err();
        assert!(err.contains("/nonexistent/sapgui"));
        assert!(!err.contains("secret"));
    }
}
