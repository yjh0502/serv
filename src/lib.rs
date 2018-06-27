extern crate bytes;
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate net2;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate http;
extern crate serde_json;
extern crate serde_qs;
extern crate tokio;
extern crate tokio_current_thread;
extern crate tokio_io;
#[cfg(feature = "uds")]
extern crate tokio_uds;
extern crate url;

pub mod error {
    use super::*;

    error_chain!{
        foreign_links {
            Hyper(hyper::Error);
            Http(http::Error);
            Io(std::io::Error);
        }

        errors {
            UnexpectedMethod(m: hyper::Method) {
                description("badarg")
            }
            UnknownMethod(m: hyper::Method) {
                description("invalid_endpoint")
            }
            DecodeJson(e: serde_json::Error) {
                description("badarg")
            }
            EncodeJson(e: serde_json::Error) {
                description("internal")
            }
            DecodeQs(e: serde_qs::Error) {
                description("badarg")
            }
        }
    }
}

type SyncObj<T> = std::rc::Rc<T>;

pub mod async;
pub mod reply;
pub mod server;
pub mod sync;

pub use error::{Error, ErrorKind};
pub use server::Server;
use std::fmt::Debug;

use futures::future::*;
use futures::*;
use hyper::header::*;
use hyper::service::Service;
use hyper::{Body, Request, Response};

pub fn resp_err() -> Response<Body> {
    Response::builder()
        .status(hyper::StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub fn resp_serv_err<E>(e: E, status: hyper::StatusCode) -> Response<Body>
where
    E: Debug + std::error::Error,
{
    let reply = reply::ServiceReply::<(), E>::from(Err(e));
    let encoded = match serde_json::to_vec(&reply) {
        Ok(v) => v,
        Err(_e) => return resp_err(),
    };

    Response::builder()
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .status(status)
        .body(Body::from(encoded))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub type HyperFuture = Box<Future<Item = Response<Body>, Error = hyper::Error>>;
pub type HyperService =
    Box<Service<ReqBody = Body, ResBody = Body, Error = hyper::Error, Future = HyperFuture>>;
pub type HyperServiceSend = Box<
    Service<
        ReqBody = Body,
        ResBody = Body,
        Error = hyper::Error,
        Future = Box<Future<Item = Response<Body>, Error = hyper::Error> + Send>,
    >,
>;

/// parse API req from qs/body
fn parse_req<R>(req: Request<Body>) -> Box<Future<Item = R, Error = Error>>
where
    R: for<'de> serde::Deserialize<'de> + 'static,
{
    use hyper::Method;
    match req.method().clone() {
        Method::GET | Method::DELETE => {
            let qs = req.uri().query().unwrap_or("");
            let req: Result<R, Error> =
                serde_qs::from_str(qs).map_err(|e| ErrorKind::DecodeQs(e).into());
            Box::new(result(req))
        }
        Method::PUT | Method::POST => {
            let buf = Vec::new();

            let f = req
                .into_body()
                .map_err(Error::from)
                .fold(buf, |mut buf, chunk| {
                    buf.extend_from_slice(&chunk);
                    //TODO: move to config?
                    if buf.len() > 1024 * 1024 * 4 {
                        Err(Error::from("body too large"))
                    } else {
                        Ok(buf)
                    }
                })
                .and_then(move |chunk: Vec<u8>| {
                    serde_json::from_slice(&chunk).map_err(|e| ErrorKind::DecodeJson(e).into())
                });
            Box::new(f)
        }
        m => Box::new(err(ErrorKind::UnexpectedMethod(m).into())),
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
pub struct Empty {}
