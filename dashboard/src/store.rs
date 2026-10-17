//! Dashboard connection store (D3): `<config root>/daiorama-dashboard/connections.ron`, 0600 in
//! a 0700 directory. One entry per manager, each carrying the instance token and the
//! `dashboard` scope secret — both are credentials and are never printed by any command.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub token: String,
    /// The reserved `dashboard` scope's network secret for this manager (D2/07). Required by
    /// every network call; stored per connection because each manager mints its own.
    #[serde(default)]
    pub scope_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connections {
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            version: 1,
            connections: Vec::new(),
        }
    }
}

/// The store path (D3): `<config root>/daiorama-dashboard/connections.ron`.
pub fn store_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        });
    config.join("daiorama-dashboard").join("connections.ron")
}

impl Connections {
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                ron::from_str(&text).map_err(|err| format!("load {}: {err}", path.display()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(format!("read {}: {err}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let Some(parent) = path.parent() else {
            return Err("store path has no parent".into());
        };
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create {}: {err}", parent.display()))?;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| format!("encode connections: {err}"))?;
        let tmp = parent.join(".connections.ron.tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|err| format!("open {}: {err}", tmp.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|err| format!("write {}: {err}", tmp.display()))?;
        file.flush().map_err(|err| format!("flush: {err}"))?;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        std::fs::rename(&tmp, path).map_err(|err| format!("replace {}: {err}", path.display()))?;
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|connection| connection.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|connection| connection.name == name)
    }
}

/// Read a secret from stdin, never argv (D3: shell history and `ps` stay clean).
pub fn read_secret() -> Result<String, String> {
    use std::io::Read;
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|err| format!("read stdin: {err}"))?;
    let value = text.trim().to_string();
    if value.is_empty() {
        return Err("empty secret; nothing written".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips_at_0600_in_a_0700_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("daiorama-dashboard")
            .join("connections.ron");
        let mut store = Connections::default();
        store.connections.push(Connection {
            name: "local".into(),
            address: "127.0.0.1:8756".into(),
            token: "tok".into(),
            scope_token: "scope".into(),
        });
        store.save(&path).unwrap();
        let loaded = Connections::load(&path).unwrap();
        assert_eq!(loaded, store);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn an_absent_store_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Connections::load(&dir.path().join("missing.ron")).unwrap();
        assert_eq!(store.version, 1);
        assert!(store.connections.is_empty());
    }
}
