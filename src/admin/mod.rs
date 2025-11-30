pub mod server {
    use flume::Sender;
    use futures::Stream;
    use serde::Deserialize;
    use tokio::{sync::mpsc, time::sleep};
    use tokio_stream::{wrappers::ReceiverStream, StreamExt};
    use tonic::{Request, Response, Status, Streaming};

    use crate::{
        proxy::RequestDebugChanels,
        proxy_proto::{
            proxy_service_server::{ProxyService, ProxyServiceServer},
            DebugMessage, DebugRequest,
        },
    };

    use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};

    use tokio::sync::RwLock;

    #[derive(Debug, Default)]
    pub struct MyGreeter {
        debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
    }

    #[derive(Debug, Deserialize)]
    struct ProxyItem {
        id: String,
        path: String,
        private: bool,
        strip_path: bool,
        host: String,
        port: i32,
    }

    async fn fetch_proxies() -> Result<Vec<ProxyItem>, reqwest::Error> {
        let resp = reqwest::get("http://localhost:9999/proxy")
            .await?
            .json::<Vec<ProxyItem>>()
            .await?;

        Ok(resp)
    }

    #[tonic::async_trait]
    impl ProxyService for MyGreeter {
        type DebugProxyStream =
            Pin<Box<dyn Stream<Item = Result<DebugMessage, tonic::Status>> + Send>>;

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
                                tx.send(Ok(req.read().await.clone().unwrap()))
                                    .await
                                    .expect("working rx");
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
        let router_clone = map.clone();
        let debug_map_clone = debug_map.clone();

        tokio::spawn(async {
            let router = router_clone;
            let debug_map = debug_map_clone;

            loop {
                match fetch_proxies().await {
                    Ok(proxies) => {
                        let mut router = router.write().await;

                        for proxy in proxies {
                            let tu = crate::TargetUpstream {
                                id: proxy.id.clone(),
                                host: proxy.host.clone(),
                                service_path: None,
                                port: proxy.port,
                                strip_path: proxy.strip_path,
                                path: proxy.path.clone(),
                                private: proxy.private,
                            };

                            let match_path = format!("{}{{*p}}", proxy.path.clone());

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

                            match router.insert(&proxy.path, tu.clone()) {
                                Err(err) => match err {
                                    matchit::InsertError::Conflict { with } => {
                                        router.remove(with);
                                        let _ = router.insert(&proxy.path, tu);
                                    }
                                    _ => {
                                        println!("Invalid route tried to be inserted {:?}", err);
                                    }
                                },
                                Ok(_) => {}
                            };

                            let debug_map_r = debug_map.read().await;
                            if debug_map_r.get(&proxy.id.clone()).is_none() {
                                drop(debug_map_r);
                                let mut debug_map = debug_map.write().await;
                                let (tx, rx) = flume::unbounded();

                                let debug_item = RequestDebugChanels {
                                    breakpoint_receiver: rx,
                                    breakpoint_sender: tx,
                                    id: Arc::new(RwLock::new("".to_string())),
                                    req: Arc::new(RwLock::new(None)),
                                };

                                debug_map.insert(proxy.id.clone(), Arc::new(debug_item));
                                drop(debug_map);
                            }

                        }


                        drop(router);
                    }
                    Err(err) => {
                        eprintln!("Error fetching proxies: {:?}", err);
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });

        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(crate::proxy_proto::FILE_DESCRIPTOR_SET)
            .build_v1alpha()
            .unwrap();

        let addr = "0.0.0.0:6189".parse().unwrap();

        let greeter = MyGreeter { debug_map };

        tonic::transport::Server::builder()
            .add_service(reflection_service)
            .add_service(ProxyServiceServer::new(greeter))
            .serve(addr)
            .await
            .unwrap();
    }
}
