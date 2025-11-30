pub mod server {
    use flume::Sender;
    use futures::Stream;
    use tokio::{sync::mpsc, time::sleep};
    use tokio_stream::{wrappers::ReceiverStream, StreamExt};
    use tonic::{Request, Response, Status, Streaming};

    use crate::{
        proxy::RequestDebugChanels,
        proxy_proto::{
            proxy_service_server::{ProxyService, ProxyServiceServer},
            DebugRequest, Empty, ProxyOperationRequest, DebugMessage
        },
    };

    use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};

    use tokio::sync::RwLock;

    #[derive(Debug, Default)]
    pub struct MyGreeter {
        map: Arc<RwLock<matchit::Router<super::super::TargetUpstream>>>,
        debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
    }

    #[tonic::async_trait]
    impl ProxyService for MyGreeter {
        type DebugProxyStream = Pin<Box<dyn Stream<Item = Result<DebugMessage, tonic::Status>> + Send>>;

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
                private: true,
            };

            let debug_item = RequestDebugChanels {
                breakpoint_receiver: rx,
                breakpoint_sender: tx,
                id: Arc::new(RwLock::new("".to_string())),
                req: Arc::new(RwLock::new(None)),
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

            let mut sender: Option<Sender<uuid::Uuid>> = None;

            tokio::spawn(async move {
                let debug_map_lock = debug_map.read().await;
                while let Some(result) = in_stream.next().await {
                    match result {
                        Ok(v) => {
                            let send_id = uuid::Uuid::new_v4();
                            if sender.is_none() {
                                sender = Some(
                                    debug_map_lock.get(&v.id).unwrap().breakpoint_sender.clone(),
                                );
                                
                                let start_body: DebugMessage;

                                loop {
                                    let req = debug_map_lock.get(&v.id).unwrap().req.clone();
                                    let ref_body = req.read().await.clone();

                                    if ref_body.is_some() {
                                        start_body = ref_body.unwrap();
                                        break;
                                    }

                                    sleep(Duration::from_millis(100)).await;
                                }

                                tx.send(Ok(start_body)).await.expect("working rx");
                                continue;
                            } else {
                                let sender = sender.as_ref().unwrap();
                                sender.send(send_id).unwrap();
                            }

                            let req = debug_map_lock.get(&v.id).unwrap().req.clone();
                            let id = debug_map_lock.get(&v.id).unwrap().id.clone();

                            let mut send_body = false;

                            loop {
                                let curr_id = id.read().await.clone();

                                if "skip" == curr_id.to_string() {
                                    break;
                                }

                                if send_id.to_string() == curr_id.to_string() {
                                    send_body = true;
                                    break;
                                }

                                sleep(Duration::from_millis(100)).await;
                            }

                            if send_body {
                                tx.send(Ok(req.read().await.clone().unwrap())).await.expect("working rx");
                            }
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }

                if sender.is_some() {
                    let sender = sender.as_ref().unwrap();
                    sender.send(uuid::Uuid::new_v4()).unwrap();
                }
            });

            let output_stream = ReceiverStream::new(rx);

            Ok(Response::new(Box::pin(output_stream)
                as Pin<
                    Box<dyn Stream<Item = Result<DebugMessage, Status>> + Send>,
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
