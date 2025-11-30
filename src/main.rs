use std::{collections::HashMap, sync::Arc, thread};

use tokio::sync::RwLock;

use tokio::runtime::Runtime;

mod admin;
mod proxy;

pub mod proxy_proto {
    tonic::include_proto!("proxy");

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("proxy_descriptor");
}

#[derive(Clone, Debug)]
pub struct TargetUpstream {
    service_path: Option<String>,
    id: String,
    strip_path: bool,
    path: String,
    host: String,
    port: i32,
    private: bool,
}

fn main() {
    let router = Arc::new(RwLock::new(matchit::Router::new()));
    let debug_map = Arc::new(RwLock::new(HashMap::new()));

    let grpc_map = router.clone();
    let grpc_debug_map = debug_map.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();

        let handle = rt.spawn(async {
            admin::server::start_server(grpc_map, grpc_debug_map).await;
        });

        rt.block_on(handle)
    });

    proxy::start_proxy(router, debug_map);
}
