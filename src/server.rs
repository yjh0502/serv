use std;
use std::rc::Rc;

use bytes::Bytes;
use futures::future::*;
use futures::*;
use h2;
use http;
use hyper;
use hyper::server::{Http, Request, Response, Service};
use regex;
use tokio_core::net::TcpListener;
use tokio_core::reactor::Handle;
use tokio_io::{AsyncRead, AsyncWrite};
use url;

use error::*;
use resp_serv_err;
use HyperService;

fn req_h2_to_h1(
    req: http::Request<h2::RecvStream>,
) -> (impl Future<Item = (), Error = Error>, hyper::Request) {
    let mut recv_stream = None;
    let (sender, body) = hyper::Body::pair();
    let req = hyper::Request::from(req.map(|_recv_stream| {
        recv_stream = Some(_recv_stream);
        body
    }));

    let f = recv_stream
        .unwrap()
        .map_err(Error::from)
        .fold(sender, |sender, bytes| {
            let chunk = hyper::Chunk::from(bytes);
            sender
                .send(Ok(chunk))
                .map_err(|_e| Error::from(format!("failed to send body: {:?}", _e)))
        })
        .map(|_sender| ());

    (f, req)
}

fn resp_h1_to_h2(
    mut respond: h2::server::SendResponse<Bytes>,
    resp: hyper::Response,
) -> impl Future<Item = (), Error = Error> {
    let mut body = None;
    let resp = http::Response::from(resp).map(|_body| {
        body = Some(_body);
        ()
    });
    let body = body.unwrap();

    result(respond.send_response(resp, false))
        .map_err(Error::from)
        .and_then(move |send_stream| {
            body.map_err(Error::from)
                .fold(send_stream, |mut sender, chunk| {
                    let bytes: Bytes = chunk.into();
                    sender.send_data(bytes, false)?;
                    Ok::<_, Error>(sender)
                })
                .and_then(|mut sender| sender.send_data(Bytes::new(), true).map_err(Error::from))
                .map(|_| ())
        })
}

fn handle_h2c(
    server: Rc<Server>,
    req: http::Request<h2::RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
) -> impl Future<Item = (), Error = Error> {
    let (f_send, req) = req_h2_to_h1(req);
    let f_call = server
        .call(req)
        .and_then(move |resp| {
            resp_h1_to_h2(respond, resp).map_err(|_e| {
                debug!("error on h2 response: {:?}", _e);
                hyper::Error::Method
            })
        })
        .map_err(Error::from);

    f_send.join(f_call).map(|_| ())
}

fn handle_sock_http1<I>(server: Rc<Server>, handle: &Handle, io: I)
where
    I: AsyncRead + AsyncWrite + 'static,
{
    let protocol = Http::<hyper::Chunk>::new();
    let f = protocol.serve_connection(io, server.clone());
    handle.spawn(f.then(|_| Ok(())));
}

fn handle_sock_h2c<I>(server: Rc<Server>, handle: &Handle, io: I)
where
    I: AsyncRead + AsyncWrite + 'static,
{
    let mut builder = h2::server::Builder::new();
    builder
        .initial_window_size(1_000_000)
        .initial_connection_window_size(100_000_000)
        .max_concurrent_streams(std::u32::MAX)
        .max_concurrent_reset_streams(1_000);

    let handle0 = handle.clone();
    let connection = builder
        .handshake(io)
        .and_then(move |conn| {
            info!("H2 connection bound");

            conn.for_each(move |(req, respond)| {
                let f = handle_h2c(server.clone(), req, respond);
                handle0.spawn(f.map_err(|e| {
                    debug!("error: {:?}", e);
                }));
                Ok(())
            })
        })
        .map(|_| ())
        .map_err(|e| {
            debug!("h2 connection error: {:?}", e);
        });

    handle.spawn(connection);
}

fn handle_incoming<S, I, F>(
    server: Rc<Server>,
    handle: Handle,
    stream: S,
    f: F,
) -> Box<Future<Item = (), Error = Error>>
where
    S: Stream<Item = I, Error = std::io::Error> + 'static,
    I: AsyncRead + AsyncWrite + 'static,
    F: Fn(Rc<Server>, &Handle, I) + 'static,
{
    let f_listen = stream
        .for_each(move |io| {
            f(server.clone(), &handle, io);
            Ok(())
        })
        .map_err(Error::from);
    Box::new(f_listen)
}

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

    #[cfg(feature = "uds")]
    pub fn run_uds(
        self,
        url: url::Url,
        handle: Handle,
        is_http2: bool,
    ) -> Box<Future<Item = (), Error = Error>> {
        let server = Rc::new(self);
        use tokio_uds;
        let handle_fn = if is_http2 {
            handle_sock_h2c
        } else {
            handle_sock_http1
        };

        let path = url.path();
        if let Err(_) = std::fs::remove_file(path) {
            //ignore error?
        }

        let listener = tokio_uds::UnixListener::bind(path).unwrap();
        return handle_incoming(server, handle, listener.incoming(), handle_fn);
    }

    #[cfg(not(feature = "uds"))]
    pub fn run_uds(
        self,
        _url: url::Url,
        _handle: Handle,
        _is_http2: bool,
    ) -> Box<Future<Item = (), Error = Error>> {
        panic!("uds not supported: {:?}", _url);
    }

    pub fn run(self, url: url::Url, handle: Handle) -> Box<Future<Item = (), Error = Error>> {
        let (is_http2, is_unix) = match url.scheme() {
            "http" => (false, false),
            "http+unix" => (false, true),
            "h2c" => (true, false),
            "h2c+unix" => (true, true),
            "" => (false, false),
            schema => {
                panic!("unexpected schema: {}", schema);
            }
        };

        if is_unix {
            self.run_uds(url, handle, is_http2)
        } else {
            let server = Rc::new(self);
            let handle_fn = if is_http2 {
                handle_sock_h2c
            } else {
                handle_sock_http1
            };

            //TODO: with_deault_port
            let addr_str = format!(
                "{}:{}",
                url.host().expect("failed to get host"),
                url.port().expect("failed to get port")
            );
            let addr = addr_str.parse().expect("failed to parse addr");

            let listener = TcpListener::bind(&addr, &handle).unwrap();
            handle_incoming(
                server,
                handle,
                listener.incoming().map(|(io, _addr)| io),
                handle_fn,
            )
        }
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
        Box::new(ok(resp_serv_err(
            Error::from("invalid_endpoint"),
            hyper::StatusCode::NotFound,
        )))
    }
}
