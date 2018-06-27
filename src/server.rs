use std;
use std::cell::RefCell;
use std::rc::Rc;

use futures::future::*;
use futures::*;
use hyper;
use hyper::{Body, Request, Response};
use regex;
use tokio::net::TcpListener;
use tokio_current_thread as current_thread;
use tokio_io::{AsyncRead, AsyncWrite};
use url;

use error::*;
use resp_serv_err;
use HyperService;
use HyperServiceSend;

fn handle_sock<I>(server: Server, io: I)
where
    I: AsyncRead + AsyncWrite + 'static,
{
    let protocol = hyper::server::conn::Http::new();
    let f = protocol.serve_connection(io, server.clone());

    current_thread::spawn(f.then(|_| Ok(())));
}

fn handle_incoming<S, I, F>(
    server: Server,
    stream: S,
    f: F,
) -> Box<Future<Item = (), Error = Error>>
where
    S: Stream<Item = I, Error = std::io::Error> + 'static,
    I: AsyncRead + AsyncWrite + 'static,
    F: Fn(Server, I) + 'static,
{
    let f_listen = stream
        .for_each(move |io| {
            f(server.clone(), io);
            Ok(())
        })
        .map_err(Error::from);
    Box::new(f_listen)
}

enum RoutePath {
    Exact(String),
    Regex(regex::Regex),
}

impl RoutePath {
    fn is_match(&self, path: &str) -> bool {
        match self {
            RoutePath::Exact(s) => path == s,
            RoutePath::Regex(re) => re.is_match(path),
        }
    }
}

enum RouteService {
    NotSend(RefCell<HyperService>),
    Send(RefCell<HyperServiceSend>),
}
impl From<HyperService> for RouteService {
    fn from(s: HyperService) -> RouteService {
        RouteService::NotSend(RefCell::new(s))
    }
}
impl From<HyperServiceSend> for RouteService {
    fn from(s: HyperServiceSend) -> RouteService {
        RouteService::Send(RefCell::new(s))
    }
}

#[derive(Default)]
pub struct Routes {
    routes: Vec<(hyper::Method, RoutePath, RouteService)>,
}
impl Routes {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    fn push_serv<S>(&mut self, method: hyper::Method, path: RoutePath, service: S)
    where
        S: Into<RouteService>,
    {
        self.routes.push((method, path, service.into()));
    }

    pub fn push(&mut self, method: hyper::Method, path: &str, service: HyperService) {
        self.push_serv(method, RoutePath::Exact(path.to_owned()), service)
    }

    pub fn push_exp(&mut self, method: hyper::Method, regexp: &str, service: HyperService) {
        let re: regex::Regex = regexp.parse().unwrap();
        self.push_serv(method, RoutePath::Regex(re), service)
    }

    pub fn push_send(&mut self, method: hyper::Method, path: &str, service: HyperServiceSend) {
        self.push_serv(method, RoutePath::Exact(path.to_owned()), service)
    }

    pub fn push_send_exp(
        &mut self,
        method: hyper::Method,
        regexp: &str,
        service: HyperServiceSend,
    ) {
        let re: regex::Regex = regexp.parse().unwrap();
        self.push_serv(method, RoutePath::Regex(re), service)
    }
}

#[derive(Default, Clone)]
pub struct Server {
    routes: Rc<Routes>,
}

impl Server {
    pub fn new(routes: Routes) -> Self {
        Self {
            routes: Rc::new(routes),
        }
    }

    #[cfg(feature = "uds")]
    pub fn run_uds(self, url: url::Url) -> Box<Future<Item = (), Error = Error>> {
        use tokio_uds;

        let path = url.path();
        if let Err(_) = std::fs::remove_file(path) {
            //ignore error?
        }

        let listener = tokio_uds::UnixListener::bind(path).unwrap();
        return handle_incoming(self, listener.incoming(), handle_sock);
    }

    #[cfg(not(feature = "uds"))]
    pub fn run_uds(self, _url: url::Url) -> Box<Future<Item = (), Error = Error>> {
        panic!("uds not supported: {:?}", _url);
    }

    pub fn run(self, url: url::Url) -> Box<Future<Item = (), Error = Error>> {
        let is_unix = match url.scheme() {
            "http" => false,
            "http+unix" => true,
            schema => {
                panic!("unexpected schema: {}", schema);
            }
        };

        if is_unix {
            self.run_uds(url)
        } else {
            //TODO: with_deault_port
            let addr_str = format!(
                "{}:{}",
                url.host().expect("failed to get host"),
                url.port().expect("failed to get port")
            );
            let addr = addr_str.parse().expect("failed to parse addr");

            let listener = TcpListener::bind(&addr).unwrap();
            handle_incoming(self, listener.incoming(), handle_sock)
        }
    }
}

impl hyper::service::Service for Server {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Response<Self::ResBody>, Error = Self::Error>>;

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method = req.method().clone();
        let uri = req.uri().clone();
        info!("req: {} {}", method, uri);

        let path = uri.path();
        for (ref route_method, ref route_path, ref serv) in self.routes.routes.iter() {
            if *route_method != method {
                continue;
            }
            if !route_path.is_match(path) {
                continue;
            }

            match serv {
                RouteService::NotSend(serv) => {
                    return serv.borrow_mut().call(req);
                }
                RouteService::Send(serv) => {
                    return serv.borrow_mut().call(req);
                }
            }
        }
        Box::new(ok(resp_serv_err(
            Error::from("invalid_endpoint"),
            hyper::StatusCode::NOT_FOUND,
        )))
    }
}

impl hyper::service::NewService for Server {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = hyper::Error;
    type Service = Self;
    type Future = FutureResult<Self::Service, Self::InitError>;
    type InitError = hyper::Error;

    fn new_service(&self) -> Self::Future {
        ok(self.clone())
    }
}
