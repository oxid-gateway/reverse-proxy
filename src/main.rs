use std::{
    collections::HashMap,
    sync::Arc,
    thread,
};

use tokio::sync::RwLock;

use tokio::runtime::Runtime;

use debug;

mod grpc;
mod proxy;
mod debug;

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
}

fn main() {
    let router = Arc::new(RwLock::new(matchit::Router::new()));
    let debug_map = Arc::new(RwLock::new(HashMap::new()));

    let grpc_map = router.clone();
    let grpc_debug_map = debug_map.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();

        let handle = rt.spawn(async {
            grpc::server::start_server(grpc_map, grpc_debug_map).await;
        });

        rt.block_on(handle)
    });

    proxy::start_proxy(router, debug_map);
}
