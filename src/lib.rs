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
            ParseInt(std::num::ParseIntError);
            SerdeJson(serde_json::Error);
            SerdeQs(serde_qs::Error);
        }

        errors {
            InvalidToken {
                description("invalid token")
            }
        }
    }
}
use error::Error;

use futures::*;
use futures::future::*;
use hyper::server::{Request, Response, Service};

pub fn resp_err() -> Response {
    hyper::server::Response::new().with_status(hyper::StatusCode::InternalServerError)
}
pub fn resp_serv_err(msg: String) -> Response {
    let resp = ServiceResp::Err::<()> { msg };
    let encoded = match serde_json::to_vec(&resp) {
        Ok(v) => v,
        Err(_e) => return resp_err(),
    };

    let body: hyper::Body = encoded.into();
    hyper::server::Response::new().with_body(body)
}

macro_rules! try_err_resp {
    ($e: expr, $msg: expr) => {
        match $e {
            Ok(v) => v,
            Err(_e) => {
                let msg = format!("error `{}`: {:?}", $msg, _e);
                return Box::new(ok(resp_serv_err(msg)));
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum ServiceResp<T: serde::Serialize> {
    #[serde(rename = "ok")]
    Ok { result: T },
    //TODO: reason
    #[serde(rename = "error")]
    Err { msg: String },
}
impl<T, E> From<Result<T, E>> for ServiceResp<T>
where
    T: serde::Serialize,
    E: std::fmt::Debug,
{
    fn from(res: Result<T, E>) -> ServiceResp<T> {
        match res {
            Ok(resp) => ServiceResp::Ok { result: resp },
            Err(e) => {
                let msg = format!("{:?}", e);
                ServiceResp::Err { msg }
            }
        }
    }
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
        Get => {
            let qs = req.uri().query().unwrap_or("");
            let req: Result<R, Error> = serde_qs::from_str(qs).map_err(Error::from);
            Box::new(result(req))
        }
        Post => {
            let f = req.body()
                .concat2()
                .map_err(Error::from)
                .and_then(move |chunk| result(serde_json::from_slice(&chunk)).map_err(Error::from));
            Box::new(f)
        }
        _ => Box::new(err("unknown method".into())),
    }
}

/// write reply body
fn reply<Resp>(resp: ServiceResp<Resp>) -> HyperFuture
where
    Resp: serde::Serialize,
{
    let encoded = try_err_resp!(serde_json::to_vec(&resp), "failed to encode resp");
    let body: hyper::Body = encoded.into();
    let resp = hyper::server::Response::new()
        .with_status(hyper::StatusCode::Ok)
        .with_body(body);

    Box::new(ok(resp))
}

mod sync;
pub use self::sync::*;

mod async;
pub use self::async::*;
