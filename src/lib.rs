extern crate bytes;
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate h2;
extern crate http;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate net2;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_qs;
extern crate tokio_core;
extern crate tokio_io;
#[cfg(feature = "uds")]
extern crate tokio_uds;
extern crate url;

pub mod error {
    use super::*;

    error_chain!{
        foreign_links {
            Hyper(hyper::Error);
            Io(std::io::Error);
        }

        errors {
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
pub mod staticfile;
pub mod sync;

pub use error::{Error, ErrorKind};
pub use server::Server;
use std::fmt::Debug;

use futures::future::*;
use futures::*;
use hyper::header::AccessControlAllowOrigin;
use hyper::server::{Request, Response, Service};

pub fn resp_err() -> Response {
    hyper::server::Response::new().with_status(hyper::StatusCode::InternalServerError)
}
pub fn resp_serv_err<E>(e: E, status: hyper::StatusCode) -> Response
where
    E: Debug + std::error::Error,
{
    let reply = reply::ServiceReply::<(), E>::from(Err(e));
    let encoded = match serde_json::to_vec(&reply) {
        Ok(v) => v,
        Err(_e) => return resp_err(),
    };

    let body: hyper::Body = encoded.into();
    hyper::server::Response::new()
        .with_header(AccessControlAllowOrigin::Any)
        .with_status(status)
        .with_body(body)
}

pub type HyperFuture = Box<Future<Item = Response, Error = hyper::Error>>;
pub type HyperService = Box<
    Service<Request = Request, Response = Response, Error = hyper::Error, Future = HyperFuture>,
>;

/// parse API req from qs/body
fn parse_req<R>(req: Request) -> Box<Future<Item = R, Error = Error>>
where
    R: for<'de> serde::Deserialize<'de> + 'static,
{
    use hyper::Method::*;
    match req.method().clone() {
        Get | Delete => {
            let qs = req.uri().query().unwrap_or("");
            let req: Result<R, Error> =
                serde_qs::from_str(qs).map_err(|e| ErrorKind::DecodeQs(e).into());
            Box::new(result(req))
        }
        Put | Post => {
            let buf = Vec::new();
            let f = req.body()
                .map_err(Error::from)
                .fold(buf, |mut buf, chunk| {
                    buf.extend_from_slice(&chunk);
                    //TODO: move to config?
                    let res = if buf.len() > 1024 * 1024 * 4 {
                        Err(Error::from("body too large"))
                    } else {
                        Ok(buf)
                    };
                    result(res)
                })
                .and_then(move |chunk| {
                    result(serde_json::from_slice(&chunk))
                        .map_err(|e| ErrorKind::DecodeJson(e).into())
                });
            Box::new(f)
        }
        method => Box::new(err(ErrorKind::UnknownMethod(method).into())),
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
pub struct Empty {}
