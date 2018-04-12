use std;
use std::rc::Rc;

use futures::future::*;
use futures::*;
use hyper;
use hyper::server::{Http, Request, Response, Service};
use regex;
use tokio_core::reactor::Handle;

use error::*;
use resp_serv_err;
use HyperService;

#[derive(Default)]
pub struct Server {
    routes: Vec<(hyper::Method, regex::Regex, HyperService)>,
}

impl Server {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    pub fn push(&mut self, method: hyper::Method, path: &str, service: HyperService) {
        let regexp = format!("^{}$", path);
        let re: regex::Regex = regexp.parse().unwrap();
        self.routes.push((method, re, service));
    }

    pub fn push_exp(&mut self, method: hyper::Method, regexp: &str, service: HyperService) {
        let re: regex::Regex = regexp.parse().unwrap();
        self.routes.push((method, re, service));
    }

    pub fn run(
        self,
        addr: std::net::SocketAddr,
        handle: Handle,
    ) -> Box<Future<Item = (), Error = Error>> {
        let server = Rc::new(self);

        let incoming = match ::listener::AddrIncoming::new(addr, handle.clone()) {
            Ok(v) => v,
            Err(e) => return Box::new(err(Error::from(e))),
        };
        let serve = Http::new()
        // use conservative value
        .max_buf_size(1024 * 16)
        .serve_incoming(incoming, move || Ok(server.clone()));

        /*
        let serve = Http::new()
            .serve_addr_handle(&addr, &handle, move || Ok(server.clone()))
            .unwrap();
            */

        let f_listen = serve
            .for_each(move |conn| {
                handle.spawn(
                    conn.map(|_| ())
                        .map_err(|err| error!("serve error: {:?}", err)),
                );
                ok(())
            })
            .into_future()
            .map_err(Error::from);

        Box::new(f_listen)
    }
}

impl Service for Server {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let method = req.method().clone();
        let uri = req.uri().clone();
        info!("req: {} {}", method, uri);

        let path = uri.path();
        for &(ref route_method, ref route_path, ref serv) in &self.routes {
            if *route_method != method {
                continue;
            }
            if !route_path.is_match(path) {
                continue;
            }
            return serv.call(req);
        }
        Box::new(ok(resp_serv_err("invalid_endpoint".to_owned())))
    }
}
