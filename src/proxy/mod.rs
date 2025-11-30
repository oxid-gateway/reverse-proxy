use std::{collections::HashMap, net::ToSocketAddrs, sync::Arc};

use tokio::sync::RwLock;

use flume::{Receiver, Sender};

use pingora::prelude::*;

use async_trait::async_trait;
use tonic::transport::Uri;

use crate::proxy_proto::DebugMessage;

#[derive(Clone)]
pub struct RequestContext {
    tu: Option<crate::TargetUpstream>,
    breakpoint_receiver: Option<Receiver<uuid::Uuid>>,
    id: Arc<RwLock<uuid::Uuid>>,
}

#[derive(Debug)]
pub struct RequestDebugChanels {
    pub breakpoint_receiver: Receiver<uuid::Uuid>,
    pub breakpoint_sender: Sender<uuid::Uuid>,
    pub id: Arc<RwLock<String>>,
    pub req: Arc<RwLock<Option<DebugMessage>>>,
}

pub struct MyProxy {
    map: Arc<RwLock<matchit::Router<crate::TargetUpstream>>>,
    debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
}

#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> RequestContext {
        return RequestContext {
            tu: None,
            breakpoint_receiver: None,
            id: Arc::new(RwLock::new(uuid::Uuid::new_v4()))
        };
    }

    async fn request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = _session.req_header().raw_path();
        let path = String::from_utf8_lossy(&path).to_string();

        let read_lock = self.map.read().await;

        match read_lock.at(&path) {
            Ok(tu) => {
                let mut debug_id: uuid::Uuid = uuid::Uuid::new_v4();
                let map = self.debug_map.read().await;
                let debug_item = map.get(&tu.value.id).unwrap();
                let receiver = debug_item.breakpoint_receiver.clone();

                if receiver.sender_count() > 1 {
                    let req_clone = debug_item.req.clone();
                    let mut req = req_clone.write().await;
                    let body = _session.read_request_body().await.unwrap();
                    let mut string_body = "".to_string();
                    if body.is_some() {
                        string_body = String::from_utf8(body.unwrap().to_vec()).unwrap();
                    }

                    let headers = _session.req_header();
                    let mut response_headers = HashMap::new();

                    for (header_name, header_value) in headers.headers.iter() {
                        response_headers.insert(
                            header_name.to_string(),
                            header_value.to_str().unwrap_or("").to_string(),
                        );
                    }

                    *req = Some(DebugMessage {
                        headers: response_headers,
                        path: path.clone(),
                        body: string_body,
                        method: headers.method.to_string(),
                        notes: "New request arrived in gateway".to_string()
                    });

                    drop(req);

                    for x in receiver.iter() {
                        debug_id = x.clone();
                        break;
                    }
                }

                let mut is_auth_error = false;

                if tu.value.private {
                    let auth = _session.req_header().headers.get("Authorization");
                    if auth.is_none() {
                        is_auth_error = true;
                    }
                }

                if receiver.sender_count() > 1 {
                    let req_clone = debug_item.req.clone();
                    let mut req = req_clone.write().await;
                    let body = _session.read_request_body().await.unwrap();
                    let mut string_body = "".to_string();
                    if body.is_some() {
                        string_body = String::from_utf8(body.unwrap().to_vec()).unwrap();
                    }

                    let headers = _session.req_header();
                    let mut response_headers = HashMap::new();

                    for (header_name, header_value) in headers.headers.iter() {
                        response_headers.insert(
                            header_name.to_string(),
                            header_value.to_str().unwrap_or("").to_string(),
                        );
                    }

                    let auth_message: String;

                    if is_auth_error {
                        auth_message = String::from("Authentication Headers from client is invalid!");
                    } else {
                        auth_message = String::from("Client authenticated successfully");
                    }

                    *req = Some(DebugMessage {
                        headers: response_headers,
                        path: path.clone(),
                        body: string_body,
                        method: headers.method.to_string(),
                        notes: auth_message
                    });

                    let id_clone = debug_item.id.clone();
                    let mut id = id_clone.write().await;
                    *id = debug_id.clone().to_string();

                    drop(req);
                    drop(id);

                    for x in receiver.iter() {
                        debug_id = x.clone();
                        break;
                    }
                }

                if is_auth_error{
                    return Err(pingora::Error::explain(HTTPStatus(401), "Missing API Key"));
                }

                _ctx.breakpoint_receiver = Some(debug_item.breakpoint_receiver.clone());
                _ctx.tu = Some(tu.value.clone());

                let id_clone = _ctx.id.clone();
                let mut id = id_clone.write().await;
                *id = debug_id;

                return Ok(false);
            }
            Err(_) => {
                return Err(pingora::Error::explain(
                    HTTPStatus(404),
                    "Route does not exist",
                ));
            }
        };
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut RequestContext,
    ) -> Result<Box<HttpPeer>> {
        let upstream = _ctx.tu.as_ref().unwrap();

        let target = format!("{}:{}", upstream.host, upstream.port)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();

        let peer = Box::new(HttpPeer::new(target, true, upstream.host.clone()));
        return Ok(peer);
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        let mut debug_id: uuid::Uuid = _ctx.id.clone().read().await.clone();
        let ur = upstream_request.clone();
        let user_agent = ur.headers.get("User-Agent").unwrap();
        let tu = _ctx.tu.clone().unwrap();
        let map = self.debug_map.read().await;
        let debug_item = map.get(&tu.id).unwrap();
        let receiver = _ctx.breakpoint_receiver.clone().unwrap();
        let path = _session.req_header().raw_path();
        let path = String::from_utf8_lossy(&path).to_string();

        upstream_request
            .insert_header("Forwarded-By", user_agent.clone().to_owned())
            .unwrap();

        upstream_request
            .insert_header("Forwarded-Proto", "http")
            .unwrap();

        upstream_request
            .insert_header("Host", tu.host.clone())
            .unwrap();

        let mut new_path = upstream_request.uri.to_string();

        if tu.strip_path {
            new_path = new_path.replace(&tu.path, "");
        }

        if tu.service_path.is_some() {
            new_path = format!("{}{}", tu.service_path.as_ref().unwrap(), new_path);
        }

        if receiver.sender_count() > 1 {
            let req_clone = debug_item.req.clone();
            let mut req = req_clone.write().await;
            let body = _session.read_request_body().await.unwrap();
            let mut string_body = "".to_string();
            if body.is_some() {
                string_body = String::from_utf8(body.unwrap().to_vec()).unwrap();
            }

            let headers = _session.req_header();
            let mut response_headers = HashMap::new();

            for (header_name, header_value) in headers.headers.iter() {
                response_headers.insert(
                    header_name.to_string(),
                    header_value.to_str().unwrap_or("").to_string(),
                );
            };

            response_headers.insert("Forwarded-By".to_string(), user_agent.clone().to_owned().to_str().unwrap().to_string());
            response_headers.insert("Forwarded-Proto".to_string(), "http".to_string());
            response_headers.insert("Host".to_string(), tu.host.clone());

            *req = Some(DebugMessage {
                headers: response_headers,
                path: path.to_string(),
                body: string_body,
                method: headers.method.to_string(),
                notes: "Headers updated to match upstream".to_string()
            });

            let id_clone = debug_item.id.clone();
            let mut id = id_clone.write().await;
            *id = debug_id.clone().to_string();

            drop(req);
            drop(id);

            for _ in receiver.iter() {
                let id_clone = debug_item.id.clone();
                let mut id = id_clone.write().await;
                *id = "skip".to_string();

                drop(id);
                break;
            }
        }

        upstream_request.set_uri(Uri::builder().path_and_query(&new_path).build().unwrap());

        Ok(())
    }
}

pub fn start_proxy(
    map: Arc<RwLock<matchit::Router<super::TargetUpstream>>>,
    debug_map: Arc<RwLock<HashMap<String, Arc<RequestDebugChanels>>>>,
) {
    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();

    let mut lb = http_proxy_service(
        &my_server.configuration,
        MyProxy {
            map: map.clone(),
            debug_map: debug_map.clone(),
        },
    );

    lb.add_tcp("0.0.0.0:6188");
    my_server.add_service(lb);
    my_server.run_forever();
}
