use std;
use std::cell::RefCell;
use std::rc::Rc;

use futures::future::*;
use futures::*;
use hyper;
use hyper::{Body, Request, Response};
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
        .map_err(|e| Error::from(e));
    Box::new(f_listen)
}

enum RoutePath {
    Exact(String),
    Prefix(String),
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

#[cfg(feature = "fst")]
type FstMap = ::fst::Map;
#[cfg(not(feature = "fst"))]
type FstMap = ();

#[derive(Default)]
pub struct Routes {
    routes: Vec<(String, RouteService)>,
    #[allow(unused)]
    map: FstMap,
}

impl Routes {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            map: Default::default(),
        }
    }

    fn push_serv<S>(&mut self, method: hyper::Method, path: RoutePath, service: S)
    where
        S: Into<RouteService>,
    {
        let key = match path {
            RoutePath::Exact(s) => format!("{}?{}?", method, s),
            RoutePath::Prefix(s) => format!("{}?{}", method, s),
        };
        self.routes.push((key, service.into()));
    }

    pub fn push(&mut self, method: hyper::Method, path: &str, service: HyperService) {
        self.push_serv(method, RoutePath::Exact(path.to_owned()), service)
    }

    pub fn push_prefix(&mut self, method: hyper::Method, prefix: &str, service: HyperService) {
        self.push_serv(method, RoutePath::Prefix(prefix.to_owned()), service)
    }

    pub fn push_send(&mut self, method: hyper::Method, path: &str, service: HyperServiceSend) {
        self.push_serv(method, RoutePath::Exact(path.to_owned()), service)
    }

    pub fn push_send_prefix(
        &mut self,
        method: hyper::Method,
        prefix: &str,
        service: HyperServiceSend,
    ) {
        self.push_serv(method, RoutePath::Prefix(prefix.to_owned()), service)
    }

    #[cfg(feature = "fst")]
    fn build(&mut self) {
        self.routes.sort_by(|(k1, _s1), (k2, _s2)| k1.cmp(k2));
        self.map = ::fst::Map::from_iter(
            self.routes
                .iter()
                .enumerate()
                .map(|(idx, (key, _serv))| (key.to_owned(), idx as u64)),
        ).expect("failed to build map");
    }

    #[cfg(feature = "fst")]
    fn longest_match(&self, key: &[u8]) -> Option<usize> {
        let fst = self.map.as_fst();
        let mut node = fst.root();
        let mut last_out = None;
        let mut out = ::fst::raw::Output::zero();
        for b in key {
            node = match node.find_input(*b) {
                None => {
                    break;
                }
                Some(i) => {
                    let t = node.transition(i);
                    out = out.cat(t.out);
                    fst.node(t.addr)
                }
            };
            if node.is_final() {
                last_out = Some(out);
            }
        }
        last_out.map(|o| o.value() as usize)
    }

    #[cfg(feature = "fst")]
    fn route(&self, method: hyper::Method, path: &str) -> Option<&RouteService> {
        let s = format!("{}?{}?", method, path);
        let idx = self.longest_match(s.as_bytes())?;
        self.routes.get(idx).map(|(_key, serv)| serv)
    }

    #[cfg(not(feature = "fst"))]
    fn build(&mut self) {}

    #[cfg(not(feature = "fst"))]
    fn route(&self, method: hyper::Method, path: &str) -> Option<&RouteService> {
        let s = format!("{}?{}?", method, path);
        for (ref key, ref route) in &self.routes {
            if *key == s {
                return Some(route);
            }
        }
        None
    }
}

#[derive(Default, Clone)]
pub struct Server {
    routes: Rc<Routes>,
}

impl Server {
    pub fn new(mut routes: Routes) -> Self {
        routes.build();
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
        if let Some(serv) = self.routes.route(method, path) {
            match serv {
                RouteService::NotSend(serv) => serv.borrow_mut().call(req),
                RouteService::Send(serv) => serv.borrow_mut().call(req),
            }
        } else {
            let e = Error::from(ErrorKind::InvalidEndpoint);
            Box::new(ok(resp_serv_err(e, hyper::StatusCode::NOT_FOUND)))
        }
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
