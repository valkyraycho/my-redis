use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

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
