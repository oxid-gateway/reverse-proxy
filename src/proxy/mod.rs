use std::{collections::HashMap, net::ToSocketAddrs, sync::Arc};

use tokio::sync::RwLock;

use flume::{Receiver, Sender};

use pingora::prelude::*;

use async_trait::async_trait;
use tonic::transport::Uri;

#[derive(Clone)]
pub struct RequestContext {
    tu: Option<crate::TargetUpstream>,
    breakpoint_receiver: Option<Receiver<bool>>,
}

#[derive(Debug)]
pub struct RequestDebugChanels {
    pub breakpoint_receiver: Receiver<bool>,
    pub breakpoint_sender: Sender<bool>,
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
        };
    }

    async fn request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = _session.req_header().raw_path();
        let path = String::from_utf8_lossy(&path).to_string();

        let read_lock = self.map.read().await;

        match read_lock.at(&path) {
            Ok(tu) => {
                let map = self.debug_map.read().await;
                let debug_item = map.get(&tu.value.id).unwrap();
                let receiver = debug_item.breakpoint_receiver.clone();

                if receiver.sender_count() > 1 {
                    for x in receiver.iter() {
                        if x {
                            break;
                        }
                    }
                }

                _ctx.breakpoint_receiver = Some(debug_item.breakpoint_receiver.clone());
                _ctx.tu = Some(tu.value.clone());

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
        let user_agent = upstream_request.headers.get("User-Agent").unwrap();
        let tu = _ctx.tu.as_ref().unwrap();

        upstream_request
            .insert_header("Forwarded-By", user_agent.to_owned())
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
