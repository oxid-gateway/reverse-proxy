use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    thread,
};

use tokio::runtime::Runtime;

mod grpc;
mod proxy;

pub mod proxy_proto {
    tonic::include_proto!("proxy");

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("proxy_descriptor");
}

#[derive(Clone, Debug)]
pub struct TargetUpstream {
    service_path: Option<String>,
    strip_path: bool,
    path: String,
    host: String,
    port: i32,
}

fn main() {
    let map = Arc::new(RwLock::new(HashMap::new()));

    let grpc_map = map.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();

        let handle = rt.spawn(async {
            grpc::server::start_server(grpc_map).await;
        });

        rt.block_on(handle)
    });

    proxy::start_proxy(map);
}
