use std::{
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
    let router = Arc::new(RwLock::new(matchit::Router::new()));

    let grpc_map = router.clone();
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();

        let handle = rt.spawn(async {
            grpc::server::start_server(grpc_map).await;
        });

        rt.block_on(handle)
    });

    proxy::start_proxy(router);
}
