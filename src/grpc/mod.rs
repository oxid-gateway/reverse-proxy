pub mod server {
    use crate::proxy_proto::proxy_service_server::{ProxyService, ProxyServiceServer};
    use crate::proxy_proto::{Empty, ProxyOperationRequest};

    use tonic::{Request, Response, Status};

    use std::{
        collections::HashMap,
        sync::{Arc, RwLock},
    };

    #[derive(Debug, Default)]
    pub struct MyGreeter {
        map: Arc<RwLock<HashMap<String, super::super::TargetUpstream>>>,
    }

    #[tonic::async_trait]
    impl ProxyService for MyGreeter {
        async fn proxy_operation(
            &self,
            request: Request<ProxyOperationRequest>,
        ) -> Result<Response<Empty>, Status> {
            let body = request.get_ref();

            let mut lock = self.map.write().unwrap();

            lock.insert(
                body.path.clone(),
                crate::TargetUpstream {
                    host: body.host.clone(),
                    port: body.port,
                    strip_path: body.strip_path,
                    path: body.path.clone(),
                },
            );

            drop(lock);

            let reply = Empty {};

            Ok(Response::new(reply))
        }
    }

    pub async fn start_server(
        map: Arc<RwLock<HashMap<String, super::super::TargetUpstream>>>,
    ) -> () {
        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(crate::proxy_proto::FILE_DESCRIPTOR_SET)
            .build_v1alpha()
            .unwrap();

        let addr = "0.0.0.0:6189".parse().unwrap();

        let greeter = MyGreeter { map };

        tonic::transport::Server::builder()
            .add_service(reflection_service)
            .add_service(ProxyServiceServer::new(greeter))
            .serve(addr)
            .await
            .unwrap();
    }
}
