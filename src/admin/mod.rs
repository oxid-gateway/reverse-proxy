pub mod server {
    use flume::Sender;
    use futures::Stream;
    use tokio::sync::mpsc;
    use tokio_stream::{wrappers::ReceiverStream, StreamExt};
    use tonic::{Request, Response, Status, Streaming};

    use crate::{
        proxy::RequestDebugChanels,
        proxy_proto::{
            proxy_service_server::{ProxyService, ProxyServiceServer},
            DebugRequest, Empty, ProxyOperationRequest,
        },
    };

    use std::{collections::HashMap, pin::Pin, sync::Arc};

    use tokio::sync::RwLock;

    #[derive(Debug, Default)]
    pub struct MyGreeter {
        map: Arc<RwLock<matchit::Router<super::super::TargetUpstream>>>,
        debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
    }

    #[tonic::async_trait]
    impl ProxyService for MyGreeter {
        type DebugProxyStream = Pin<Box<dyn Stream<Item = Result<Empty, tonic::Status>> + Send>>;

        async fn proxy_operation(
            &self,
            request: Request<ProxyOperationRequest>,
        ) -> Result<Response<Empty>, Status> {
            let body = request.get_ref();
            let mut router = self.map.write().await;
            let mut debug_map = self.debug_map.write().await;
            let match_path = format!("{}{{*p}}", body.path.clone());

            if body.path.contains("{*p}") {
                return Err(Status::invalid_argument("Path cannot contain {*p}"));
            }

            if !body.path.starts_with("/") {
                return Err(Status::invalid_argument("Path needs to start with /"));
            }

            let (tx, rx) = flume::unbounded();

            let tu = crate::TargetUpstream {
                id: body.id.clone(),
                host: body.host.clone(),
                service_path: body.service_path.clone(),
                port: body.port,
                strip_path: body.strip_path,
                path: body.path.clone(),
            };

            let debug_item = RequestDebugChanels {
                breakpoint_receiver: rx,
                breakpoint_sender: tx,
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
                Ok(_) => {}
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
                Ok(_) => {}
            };

            debug_map.insert(body.id.clone(), Arc::new(debug_item));

            drop(router);

            let reply = Empty {};

            Ok(Response::new(reply))
        }

        async fn debug_proxy(
            &self,
            request: Request<Streaming<DebugRequest>>,
        ) -> Result<Response<Self::DebugProxyStream>, Status> {
            let debug_map = self.debug_map.clone();
            let mut in_stream = request.into_inner();
            let (tx, rx) = mpsc::channel(10);

            let mut sender: Option<Sender<bool>> = None;

            tokio::spawn(async move {
                let debug_map_lock = debug_map.read().await;
                while let Some(result) = in_stream.next().await {
                    match result {
                        Ok(v) => {
                            if sender.is_none() {
                                sender = Some(
                                    debug_map_lock.get(&v.id).unwrap().breakpoint_sender.clone(),
                                );

                                continue;
                            } else {
                                let sender = sender.as_ref().unwrap();
                                sender.send(true).unwrap();
                            }

                            tx.send(Ok(Empty {})).await.expect("working rx");
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }

                if sender.is_some() {
                    let sender = sender.as_ref().unwrap();
                    sender.send(true).unwrap();
                }
            });

            let output_stream = ReceiverStream::new(rx);

            Ok(Response::new(Box::pin(output_stream)
                as Pin<
                    Box<dyn Stream<Item = Result<Empty, Status>> + Send>,
                >))
        }
    }

    pub async fn start_server(
        map: Arc<RwLock<matchit::Router<super::super::TargetUpstream>>>,
        debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
    ) -> () {
        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(crate::proxy_proto::FILE_DESCRIPTOR_SET)
            .build_v1alpha()
            .unwrap();

        let addr = "0.0.0.0:6189".parse().unwrap();

        let greeter = MyGreeter { map, debug_map };

        tonic::transport::Server::builder()
            .add_service(reflection_service)
            .add_service(ProxyServiceServer::new(greeter))
            .serve(addr)
            .await
            .unwrap();
    }
}
