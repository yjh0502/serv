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

fn handle_h2c(
    server: Rc<Server>,
    handle: Handle,
    req: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
) {
    eprintln!("req: {:?}", req);

    let req = hyper::Request::from(req.map(|recv_stream| {
        let (sender, body) = hyper::Body::pair();

        let f = recv_stream
            .map_err(|_e| {
                //TODO
                ()
            })
            .fold(sender, |sender, bytes| {
                eprintln!("data< {:?}", bytes);
                let chunk = hyper::Chunk::from(bytes);
                sender.send(Ok(chunk)).map_err(|_e| {
                    //TODO
                    ()
                })
            })
            .map_err(|_e| {
                //TODO: error handling
                eprintln!("error on recv: {:?}", _e);
            })
            .map(|_sender| ());
        handle.spawn(f);

        body
    }));

    let f = server
        .call(req)
        .and_then(
            move |resp| -> Box<Future<Item = (), Error = hyper::Error>> {
                eprintln!("resp: {:?}", resp);

                let mut body = None;
                let resp = http::Response::from(resp).map(|_body| {
                    body = Some(_body);
                    ()
                });

                let send_stream = match respond.send_response(resp, false) {
                    Ok(s) => s,
                    Err(_e) => {
                        eprintln!("error on send_response: {:?}", _e);
                        return Box::new(err(hyper::Error::Method));
                    }
                };
                let body = body.unwrap_or_default();

                let f = body.map_err(|_e| {
                    eprintln!("error on resp body: {:?}", _e);
                    h2::Reason::NO_ERROR.into()
                }).fold(send_stream, |mut sender, chunk| {
                        eprintln!("data> {:?}", chunk);
                        //TODO
                        sender.send_data(chunk.into(), false)?;
                        Ok::<_, h2::Error>(sender)
                    })
                    .and_then(|mut sender| sender.send_data(Vec::new().into(), true));

                Box::new(f.map_err(|_e| {
                    eprintln!("error on resp body: {:?}", _e);
                    hyper::Error::Method
                }))
            },
        )
        .map_err(|_e| {
            eprintln!("error on call: {:?}", _e);
            ()
        });

    handle.spawn(f)
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
    let handle0 = handle.clone();
    let connection = h2::server::handshake(io)
        .and_then(move |conn| {
            info!("H2 connection bound");

            conn.for_each(move |(req, respond)| {
                handle_h2c(server.clone(), handle0.clone(), req, respond);
                Ok(())
            })
        })
        .and_then(|_| Ok(()))
        .then(|res| {
            if let Err(e) = res {
                info!("  -> err={:?}", e);
            } else {
                info!("closed");
            }

            Ok(())
        });

    handle.spawn(connection);
}

fn handle_incoming<S, I, A, F>(
    server: Rc<Server>,
    handle: Handle,
    stream: S,
    f: F,
) -> Box<Future<Item = (), Error = Error>>
where
    S: Stream<Item = (I, A), Error = std::io::Error> + 'static,
    I: AsyncRead + AsyncWrite + 'static,
    A: 'static,
    F: Fn(Rc<Server>, &Handle, I) + 'static,
{
    let f_listen = stream
        .for_each(move |(sock, _)| {
            f(server.clone(), &handle, sock);
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

        let listener = tokio_uds::UnixListener::bind(path, &handle).unwrap();
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
            handle_incoming(server, handle, listener.incoming(), handle_fn)
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
            "invalid_endpoint".to_owned(),
            hyper::StatusCode::NotFound,
        )))
    }
}
