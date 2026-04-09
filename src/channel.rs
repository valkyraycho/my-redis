use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::broadcast;

pub type Channels = Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>;
