use std::collections::HashMap;

use futures::lock::Mutex;
use tokio::sync::Mutex;

use flume::{Receiver, Sender};

struct RouteDebugConnection {
    occupied: Arc<Mutex<bool>>,
    breakpoint_receiver: Receiver<bool>,
    breakpoint_sender: Sender<bool>,
}

struct RouteDebugger {
}

impl RouteDebugger {
    fn attach(&self, id: &str) -> bool {
        let occupied = self.occupied.clone().lock();

        if occupied {
            return false
        }

        occupied = true;
        drop(occupied)
    }
}
