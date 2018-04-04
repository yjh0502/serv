use futures::future::*;
use futures::*;
use hyper;
use hyper::server::{Request, Response, Service};
use regex;

use HyperService;
use resp_serv_err;

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
