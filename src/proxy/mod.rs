use std::{
    collections::HashMap,
    net::ToSocketAddrs,
    sync::{Arc, RwLock},
};

use pingora::prelude::*;

use async_trait::async_trait;
use tonic::transport::Uri;

#[derive(Clone)]
pub struct RequestContext {
    tu: Option<crate::TargetUpstream>,
}

pub struct MyProxy {
    map: Arc<RwLock<HashMap<String, crate::TargetUpstream>>>,
}

#[async_trait]
impl ProxyHttp for MyProxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> RequestContext {
        return RequestContext { tu: None };
    }

    async fn request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = _session.req_header().raw_path();
        let path = String::from_utf8_lossy(&path).to_string();

        let read_lock = self.map.read().unwrap();
        match read_lock.get(&path) {
            Some(tu) => {
                _ctx.tu = Some(tu.clone());
                return Ok(false);
            }
            None => {
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

        if tu.strip_path {
            let new_path = upstream_request.uri.to_string().replace(&tu.path, "");
            upstream_request.set_uri(Uri::builder().path_and_query(new_path).build().unwrap());
        }

        Ok(())
    }
}

pub fn start_proxy(map: Arc<RwLock<HashMap<String, super::TargetUpstream>>>) {
    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();

    let mut lb = http_proxy_service(&my_server.configuration, MyProxy { map: map.clone() });

    lb.add_tcp("0.0.0.0:6188");
    my_server.add_service(lb);
    my_server.run_forever();
}
