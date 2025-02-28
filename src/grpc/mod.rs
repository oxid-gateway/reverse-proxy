pub mod server {
    use crate::proxy_proto::proxy_service_server::{ProxyService, ProxyServiceServer};
    use crate::proxy_proto::{Empty, ProxyOperationRequest};

    use tonic::{Request, Response, Status};

    use std::sync::{Arc, RwLock};

    #[derive(Debug, Default)]
    pub struct MyGreeter {
        map: Arc<RwLock<matchit::Router<super::super::TargetUpstream>>>,
    }

    #[tonic::async_trait]
    impl ProxyService for MyGreeter {
        async fn proxy_operation(
            &self,
            request: Request<ProxyOperationRequest>,
        ) -> Result<Response<Empty>, Status> {
            let body = request.get_ref();
            let mut router = self.map.write().unwrap();
            let match_path = format!("{}{{*p}}", body.path.clone());

            if body.path.contains("{*}") {
                return Err(Status::invalid_argument("Path cannot contain {*p}"))
            }

            if !body.path.starts_with("/") {
                return Err(Status::invalid_argument("Path needs to start with /"))
            }

            let tu = crate::TargetUpstream {
                host: body.host.clone(),
                service_path: body.service_path.clone(),
                port: body.port,
                strip_path: body.strip_path,
                path: body.path.clone(),
            };

            match router.insert(&match_path, tu.clone()) {
                Err(err) => match err {
                    matchit::InsertError::Conflict { with } => {
                        router.remove(with);
                        let _ = router.insert(match_path, tu.clone());
                    }
                    _ => {
                        println!("Invalid route tried to be inserted {:?}", err);
                    }
                },
                Ok(_) => {
                }
            };

            match router.insert(&body.path, tu.clone()) {
                Err(err) => match err {
                    matchit::InsertError::Conflict { with } => {
                        router.remove(with);
                        let _ = router.insert(&body.path, tu);
                    }
                    _ => {
                        println!("Invalid route tried to be inserted {:?}", err);
                    }
                },
                Ok(_) => {
                }
            };

            drop(router);

            let reply = Empty {};

            Ok(Response::new(reply))
        }
    }

    pub async fn start_server(
        map: Arc<RwLock<matchit::Router<super::super::TargetUpstream>>>,
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
