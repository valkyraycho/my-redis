use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use bincode;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
pub type Store = Arc<Mutex<HashMap<String, Entry>>>;

pub struct Entry {
    pub value: String,
    pub expires_at: Option<Instant>,
}

impl Entry {
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            None => false,
            Some(expires_at) => Instant::now() > expires_at,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedEntry {
    value: String,
    ttl_secs: Option<f64>,
}

impl From<&Entry> for SavedEntry {
    fn from(entry: &Entry) -> Self {
        let remaining = entry
            .expires_at
            .and_then(|exp| exp.checked_duration_since(Instant::now()));

        Self {
            value: entry.value.clone(),
            ttl_secs: remaining.map(|d| d.as_secs_f64()),
        }
    }
}

impl From<SavedEntry> for Entry {
    fn from(entry: SavedEntry) -> Self {
        Self {
            value: entry.value,
            expires_at: entry
                .ttl_secs
                .map(|secs| Instant::now() + Duration::from_secs_f64(secs)),
        }
    }
}

pub fn save(db: &Store, path: &Path) -> std::io::Result<()> {
    let db = db.lock().unwrap();
    let data_to_be_saved = db
        .iter()
        .filter(|(_, entry)| !entry.is_expired())
        .map(|(key, entry)| (key.clone(), entry.into()))
        .collect::<HashMap<String, SavedEntry>>();

    let bytes_to_store =
        bincode::serde::encode_to_vec(&data_to_be_saved, bincode::config::standard())
            .map_err(std::io::Error::other)?;
    std::fs::write(path, bytes_to_store)?;
    Ok(())
}

pub fn load(path: &Path) -> std::io::Result<Store> {
    let bytes = std::fs::read(path)?;
    let (data, _): (HashMap<String, SavedEntry>, _) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
            .map_err(std::io::Error::other)?;

    let store: HashMap<String, Entry> = data
        .into_iter()
        .map(|(key, saved_entry)| (key, Entry::from(saved_entry)))
        .collect();

    Ok(Arc::new(Mutex::new(store)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    fn new_store() -> Store {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn save_and_load_basic() {
        let store = new_store();
        {
            let mut db = store.lock().unwrap();
            db.insert(
                "name".to_string(),
                Entry {
                    value: "ray".to_string(),
                    expires_at: None,
                },
            );
            db.insert(
                "age".to_string(),
                Entry {
                    value: "25".to_string(),
                    expires_at: None,
                },
            );
        }

        let tmp = NamedTempFile::new().unwrap();
        save(&store, tmp.path()).unwrap();

        let loaded = load(tmp.path()).unwrap();
        let db = loaded.lock().unwrap();
        assert_eq!(db.get("name").unwrap().value, "ray");
        assert_eq!(db.get("age").unwrap().value, "25");
    }

    #[test]
    fn save_skips_expired_entries() {
        let store = new_store();
        {
            let mut db = store.lock().unwrap();
            db.insert(
                "alive".to_string(),
                Entry {
                    value: "yes".to_string(),
                    expires_at: None,
                },
            );
            db.insert(
                "dead".to_string(),
                Entry {
                    value: "no".to_string(),
                    expires_at: Some(Instant::now() - Duration::from_secs(10)),
                },
            );
        }

        let tmp = NamedTempFile::new().unwrap();
        save(&store, tmp.path()).unwrap();

        let loaded = load(tmp.path()).unwrap();
        let db = loaded.lock().unwrap();
        assert_eq!(db.get("alive").unwrap().value, "yes");
        assert!(db.get("dead").is_none());
    }

    #[test]
    fn save_preserves_ttl() {
        let store = new_store();
        {
            let mut db = store.lock().unwrap();
            db.insert(
                "key".to_string(),
                Entry {
                    value: "val".to_string(),
                    expires_at: Some(Instant::now() + Duration::from_secs(60)),
                },
            );
        }

        let tmp = NamedTempFile::new().unwrap();
        save(&store, tmp.path()).unwrap();

        let loaded = load(tmp.path()).unwrap();
        let db = loaded.lock().unwrap();
        let entry = db.get("key").unwrap();
        assert_eq!(entry.value, "val");
        assert!(entry.expires_at.is_some());
        assert!(!entry.is_expired());
    }

    #[test]
    fn load_missing_file_returns_error() {
        let result = load(Path::new("nonexistent.rdb"));
        assert!(result.is_err());
    }

    #[test]
    fn save_empty_store() {
        let store = new_store();
        let tmp = NamedTempFile::new().unwrap();
        save(&store, tmp.path()).unwrap();

        let loaded = load(tmp.path()).unwrap();
        let db = loaded.lock().unwrap();
        assert!(db.is_empty());
    }
}
