#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate hyper;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_qs;

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

pub mod sync;
pub mod async;
pub mod reply;

use std::fmt::{Debug, Display};

pub use error::{Error, ErrorKind};

use futures::*;
use futures::future::*;
use hyper::server::{Request, Response, Service};

pub fn resp_err() -> Response {
    hyper::server::Response::new().with_status(hyper::StatusCode::InternalServerError)
}
pub fn resp_serv_err<E>(e: E) -> Response
where
    E: Debug + Display,
{
    let reply = reply::ServiceReply::<(), E>::from(Err(e));
    let encoded = match serde_json::to_vec(&reply) {
        Ok(v) => v,
        Err(_e) => return resp_err(),
    };

    let body: hyper::Body = encoded.into();
    hyper::server::Response::new().with_body(body)
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
            let f = req.body()
                .concat2()
                .map_err(Error::from)
                .and_then(move |chunk| {
                    result(serde_json::from_slice(&chunk))
                        .map_err(|e| ErrorKind::DecodeJson(e).into())
                });
            Box::new(f)
        }
        method => Box::new(err(ErrorKind::UnknownMethod(method).into())),
    }
}

#[derive(Serialize, Deserialize)]
pub struct Empty {}
