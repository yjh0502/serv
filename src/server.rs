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

use error::*;
use resp_serv_err;
use HyperService;

#[derive(Default)]
pub struct Server {
    routes: Vec<(hyper::Method, regex::Regex, HyperService)>,
}

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

    pub fn run_h2c(
        self,
        addr: std::net::SocketAddr,
        handle: Handle,
    ) -> Box<Future<Item = (), Error = Error>> {
        let server = Rc::new(self);
        let listener = TcpListener::bind(&addr, &handle).unwrap();

        let f_listen = listener.incoming().for_each(move |(socket, _)| {
            // let socket = io_dump::Dump::to_stdout(socket);

            let server = server.clone();
            let handle0 = handle.clone();
            let connection = h2::server::handshake(socket)
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
            Ok(())
        });

        Box::new(f_listen.map_err(Error::from))
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
