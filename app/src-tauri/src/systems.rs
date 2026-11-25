use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Logon {
    pub id: String,
    pub client: String,
    pub user: String,
    pub language: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct System {
    pub id: String,
    pub name: String,
    pub sid: String,
    pub host: String,
    pub sysnr: String,
    pub logons: Vec<Logon>,
    /// Folder path from the SAP Logon landscape, outermost first. Empty means top level.
    pub folder: Vec<String>,
    /// The user chose the folder, so import leaves it alone.
    pub folder_moved: bool,
    pub notes: String,
    pub environment: String,
    pub favorite: bool,
    /// Unix time in seconds of the last launch; 0 = never.
    pub last_used: u64,
    // Single-logon fields of files saved before v1.0; read once, moved into `logons` by `load`.
    #[serde(skip_serializing)]
    pub(crate) client: String,
    #[serde(skip_serializing)]
    pub(crate) user: String,
    #[serde(skip_serializing)]
    pub(crate) language: String,
    #[serde(skip_serializing)]
    pub(crate) password: String,
}

impl System {
    fn migrate(&mut self) {
        // Short labels used before the full names.
        let env = match self.environment.as_str() {
            "DEV" => "Development",
            "TEST" | "Quality" | "Quality Assurance" => "QA",
            "PROD" => "Production",
            other => other,
        };
        self.environment = env.to_string();

        let old = Logon {
            id: format!("{}-1", self.id),
            client: std::mem::take(&mut self.client),
            user: std::mem::take(&mut self.user),
            language: std::mem::take(&mut self.language),
            password: std::mem::take(&mut self.password),
        };
        if self.logons.is_empty() && !(old.client.is_empty() && old.user.is_empty() && old.password.is_empty()) {
            self.logons.push(old);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub sapgui_path: String,
    pub systems: Vec<System>,
    /// All folders, imported or added by the user; they stay when empty.
    pub folders: Vec<Vec<String>>,
}

impl Store {
    pub fn load(path: &Path) -> io::Result<Store> {
        match fs::read_to_string(path) {
            Ok(s) => {
                let mut store: Store = serde_json::from_str(&s).map_err(io::Error::other)?;
                store.systems.iter_mut().for_each(System::migrate);
                store.store_folders();
                Ok(store)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Store::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        // Write a temp file, then rename, so a crash never leaves a half-written store.
        let tmp = path.with_extension("json.tmp");
        let _ = fs::remove_file(&tmp);
        write_private(&tmp, json.as_bytes())?;
        fs::rename(&tmp, path)
    }

    pub fn upsert(&mut self, mut system: System) {
        match self.systems.iter_mut().find(|s| s.id == system.id) {
            Some(existing) => {
                system.folder_moved = existing.folder_moved || existing.folder != system.folder;
                *existing = system;
            }
            None => self.systems.push(system),
        }
        self.store_folders();
    }

    fn folder_paths(&self) -> impl Iterator<Item = &Vec<String>> {
        self.folders.iter().chain(self.systems.iter().map(|s| &s.folder))
    }

    pub fn add_folder(&mut self, path: Vec<String>) -> Result<(), String> {
        let path = self.new_folder_path(&path)?;
        self.folders.push(path);
        Ok(())
    }

    /// Renames a folder or moves it into another one, with everything in it.
    pub fn move_folder(&mut self, from: &[String], to: Vec<String>) -> Result<(), String> {
        if from.is_empty() || !self.has_folder(from) {
            return Err("Folder not found.".into());
        }
        if to == from {
            return Ok(());
        }
        if to.len() > from.len() && to.starts_with(from) {
            return Err("A folder cannot be moved into itself. Choose another folder.".into());
        }
        let to = self.new_folder_path(&to)?;
        let rebase = |path: &mut Vec<String>| {
            let inside = path.starts_with(from);
            if inside {
                path.splice(..from.len(), to.iter().cloned());
            }
            inside
        };
        self.folders.iter_mut().for_each(|f| {
            rebase(f);
        });
        for s in &mut self.systems {
            s.folder_moved |= rebase(&mut s.folder);
        }
        self.keep_folder(&from[..from.len() - 1]);
        Ok(())
    }

    fn new_folder_path(&self, path: &[String]) -> Result<Vec<String>, String> {
        let path: Vec<String> = path.iter().map(|name| name.trim().to_string()).collect();
        if path.is_empty() || path.iter().any(String::is_empty) {
            return Err("Enter a folder name.".into());
        }
        if self.has_folder(&path) {
            return Err(format!("Folder \"{}\" already exists.", path.join(" / ")));
        }
        Ok(path)
    }

    fn has_folder(&self, path: &[String]) -> bool {
        self.folder_paths().any(|p| p.starts_with(path))
    }

    /// Puts every folder that holds a system into `folders`, once.
    fn store_folders(&mut self) {
        let mut all: Vec<Vec<String>> = Vec::new();
        for path in self.folder_paths().filter(|p| !p.is_empty()) {
            if !all.contains(path) {
                all.push(path.clone());
            }
        }
        self.folders = all;
    }

    /// Stores a folder that would disappear because its last content was taken out.
    fn keep_folder(&mut self, path: &[String]) {
        if !path.is_empty() && !self.has_folder(path) {
            self.folders.push(path.to_vec());
        }
    }

    /// Removes a folder. Its systems and folders move to the parent, and import does not move them back.
    pub fn delete_folder(&mut self, path: &[String]) -> Result<(), String> {
        let Some(last) = path.len().checked_sub(1).filter(|_| self.has_folder(path)) else {
            return Err("Folder not found.".into());
        };
        self.folders.retain(|f| f != path);
        for f in self.folders.iter_mut().filter(|f| f.starts_with(path)) {
            f.remove(last);
        }
        for s in self.systems.iter_mut().filter(|s| s.folder.starts_with(path)) {
            s.folder.remove(last);
            s.folder_moved = true;
        }
        self.store_folders();
        self.keep_folder(&path[..last]);
        Ok(())
    }

    /// Adds systems whose id is not stored yet; for stored ones only the folder is updated,
    /// so the user's credentials stay. A folder the user chose stays too. Returns how many were added.
    pub fn merge(&mut self, imported: Vec<System>) -> usize {
        let before = self.systems.len();
        for s in imported {
            match self.systems.iter_mut().find(|e| e.id == s.id) {
                Some(existing) if existing.folder_moved => {}
                Some(existing) => existing.folder = s.folder,
                None => self.systems.push(s),
            }
        }
        self.store_folders();
        self.systems.len() - before
    }
}

#[cfg(unix)]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(data)
}

#[cfg(not(unix))]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    fs::write(path, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(id: &str, client: &str) -> System {
        let logons = vec![Logon { id: "l".into(), client: client.into(), ..Default::default() }];
        System { id: id.into(), logons, ..Default::default() }
    }

    #[test]
    fn old_single_logon_file_moves_into_logons() {
        let json = r#"{"systems":[{"id":"a","client":"100","user":"U","language":"EN","password":"p"},{"id":"b"}]}"#;
        let dir = std::env::temp_dir().join(format!("sapgui-launcher-migrate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("systems.json");
        fs::write(&path, json).unwrap();
        let store = Store::load(&path).unwrap();
        let l = &store.systems[0].logons;
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].id.as_str(), l[0].client.as_str(), l[0].user.as_str(), l[0].password.as_str()), ("a-1", "100", "U", "p"));
        assert!(store.systems[1].logons.is_empty());
        store.save(&path).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("\"client\":\"100\",\"user\""));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_load_roundtrip_and_missing_file() {
        let dir = std::env::temp_dir().join(format!("sapgui-launcher-test-{}", std::process::id()));
        let path = dir.join("systems.json");
        assert_eq!(Store::load(&path).unwrap(), Store::default());

        let store = Store { sapgui_path: "/x".into(), systems: vec![sys("a", "100")], folders: vec![vec!["F".into()]] };
        store.save(&path).unwrap();
        store.save(&path).unwrap(); // second save must replace, not fail
        assert_eq!(Store::load(&path).unwrap(), store);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_are_added_once() {
        let path = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let mut store = Store::default();
        store.upsert(System { folder: path(&["A", "B"]), ..sys("a", "100") });
        assert!(store.add_folder(path(&["  "])).is_err());
        assert!(store.add_folder(path(&["A"])).is_err());
        store.add_folder(path(&["A", " C "])).unwrap();
        assert!(store.add_folder(path(&["A", "C"])).is_err());
        assert_eq!(store.folders, vec![path(&["A", "B"]), path(&["A", "C"])]);
    }

    #[test]
    fn imported_folder_is_deleted_without_losing_systems() {
        let path = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let import = || vec![
            System { folder: path(&["A", "B"]), ..sys("a", "100") },
            System { folder: path(&["A", "B", "C"]), ..sys("b", "100") },
        ];
        let mut store = Store::default();
        store.merge(import());
        assert_eq!(store.folders, vec![path(&["A", "B"]), path(&["A", "B", "C"])]);
        assert!(store.delete_folder(&path(&["No"])).is_err());
        assert!(store.delete_folder(&[]).is_err());

        store.delete_folder(&path(&["A", "B"])).unwrap();
        // the next import must not bring the folder back
        store.merge(import());
        assert_eq!(store.systems.iter().map(|s| s.folder.clone()).collect::<Vec<_>>(), vec![path(&["A"]), path(&["A", "C"])]);
        assert_eq!(store.folders, vec![path(&["A", "C"]), path(&["A"])]);

        store.delete_folder(&path(&["A", "C"])).unwrap();
        store.delete_folder(&path(&["A"])).unwrap();
        assert!(store.folders.is_empty());
        assert_eq!(store.systems.len(), 2);

        store.add_folder(path(&["Y", "Z"])).unwrap();
        store.delete_folder(&path(&["Y", "Z"])).unwrap();
        assert_eq!(store.folders, vec![path(&["Y"])]);
    }

    #[test]
    fn folder_moves_with_its_content() {
        let path = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let mut store = Store { folders: vec![path(&["A", "Empty"]), path(&["Z"])], ..Default::default() };
        store.upsert(System { folder: path(&["A", "B"]), ..sys("a", "100") });
        assert!(store.move_folder(&path(&["No"]), path(&["X"])).is_err());
        assert!(store.move_folder(&path(&["A"]), path(&["A", "B", "A"])).is_err());
        assert!(store.move_folder(&path(&["A"]), path(&["Z"])).is_err());
        assert!(store.move_folder(&path(&["A"]), path(&[" "])).is_err());
        store.move_folder(&path(&["A"]), path(&["A"])).unwrap();
        assert!(!store.systems[0].folder_moved);

        store.move_folder(&path(&["A"]), path(&["Z", " New "])).unwrap();
        assert_eq!(store.systems[0].folder, path(&["Z", "New", "B"]));
        assert!(store.systems[0].folder_moved);
        assert_eq!(store.folders, vec![path(&["Z", "New", "Empty"]), path(&["Z"]), path(&["Z", "New", "B"])]);

        // a folder stays when its last system leaves
        store.upsert(sys("a", "100"));
        assert!(store.folders.contains(&path(&["Z", "New", "B"])));
    }

    #[test]
    fn import_keeps_a_folder_the_user_chose() {
        let mut store = Store { systems: vec![sys("a", "100")], ..Default::default() };
        store.upsert(System { folder: vec!["Mine".into()], ..sys("a", "100") });
        store.merge(vec![System { id: "a".into(), folder: vec!["SAP".into()], ..Default::default() }]);
        assert_eq!(store.systems[0].folder, ["Mine"]);
    }

    #[test]
    fn upsert_and_merge() {
        let mut store = Store::default();
        store.upsert(sys("a", "100"));
        store.upsert(sys("a", "200"));
        assert_eq!(store.systems, vec![sys("a", "200")]);

        let moved = System { folder: vec!["F".into()], ..System { id: "a".into(), ..Default::default() } };
        assert_eq!(store.merge(vec![moved, sys("b", "")]), 1);
        let kept = System { folder: vec!["F".into()], ..sys("a", "200") };
        assert_eq!(store.systems, vec![kept, sys("b", "")]);
    }
}
