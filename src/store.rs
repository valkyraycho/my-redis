use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub type Store = Arc<Mutex<HashMap<String, String>>>;
