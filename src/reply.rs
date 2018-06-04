use super::*;

use async::*;
use hyper::header::{AccessControlAllowOrigin, CacheControl, CacheDirective, ContentLength,
                    ContentType, Headers};
use std::convert::From;

/// Oneshot-style reply which contains response or error.
pub trait Reply<T, E>: serde::Serialize + From<Result<T, E>>
where
    T: serde::Serialize + 'static,
    E: From<Error> + 'static,
{
    /// write reply body
    fn reply(&self, status: hyper::StatusCode) -> HyperFuture {
        let encoded = match serde_json::to_vec(&self) {
            Ok(encoded) => encoded,
            Err(e) => {
                return Box::new(ok(resp_serv_err::<Error>(
                    ErrorKind::EncodeJson(e).into(),
                    hyper::StatusCode::Ok,
                )));
            }
        };

        let mut headers = Headers::new();
        headers.set(AccessControlAllowOrigin::Any); //TODO
        headers.set(CacheControl(vec![
            CacheDirective::NoCache,
            CacheDirective::NoStore,
            CacheDirective::MustRevalidate,
        ]));
        headers.set(ContentType::json());
        headers.set(ContentLength(encoded.len() as u64));

        let body: hyper::Body = encoded.into();

        let resp = hyper::server::Response::new()
            .with_headers(headers)
            .with_status(status)
            .with_body(body);

        Box::new(ok(resp))
    }

    /// `serv_state` build `HyperService` with given function `F` and state `S`.
    fn serv_state<F, S, Req>(state: S, f: F) -> HyperService
    where
        Self: 'static,
        F: for<'a> Fn(&'a S, Req) -> Box<Future<Item = T, Error = E>> + 'static,
        S: 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = AsyncServiceFn::new(move |req| f(&state, req));
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `service` builds `HyperService` with given function `F`.
    fn serv<F, Req>(f: F) -> HyperService
    where
        Self: 'static,
        F: Fn(Req) -> Box<Future<Item = T, Error = E>> + 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = AsyncServiceFn::new(f);
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `serv_state` builds `HyperService` with given function `F` and state `S`.
    fn serv_state_sync<F, S, Req>(state: S, f: F) -> HyperService
    where
        Self: 'static,
        F: for<'a> Fn(&'a S, Req) -> Result<T, E> + 'static,
        S: 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = async::AsyncServiceFn::new(move |req| Box::new(result(f(&state, req))));
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `serv` build `HyperService` with given function `F`.
    fn serv_sync<F, Req>(f: F) -> HyperService
    where
        Self: 'static,
        F: Fn(Req) -> Result<T, E> + 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = async::AsyncServiceFn::new(move |req| Box::new(result(f(req))));
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum ServiceReply<T: serde::Serialize, E> {
    #[serde(rename = "ok")]
    Ok { result: T },
    //TODO: reason
    #[serde(rename = "error")]
    Err {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
        #[serde(skip)]
        _e: E,
    },
}

impl<T, E> From<E> for ServiceReply<T, E>
where
    T: serde::Serialize,
    E: Debug + std::error::Error,
{
    #[cfg(debug_assertions)]
    fn from(e: E) -> ServiceReply<T, E> {
        let reason = e.description().to_owned();
        let msg = format!("{:?}", e);
        ServiceReply::Err {
            reason,
            msg: Some(msg),
            _e: e,
        }
    }

    #[cfg(not(debug_assertions))]
    fn from(e: E) -> ServiceReply<T, E> {
        let reason = e.description().to_owned();
        ServiceReply::Err {
            reason,
            msg: None,
            _e: e,
        }
    }
}

impl<T, E> From<Result<T, E>> for ServiceReply<T, E>
where
    T: serde::Serialize,
    E: Debug + std::error::Error,
{
    fn from(res: Result<T, E>) -> ServiceReply<T, E> {
        match res {
            Ok(resp) => ServiceReply::Ok { result: resp },
            Err(e) => {
                trace!("error: {:?}", e);
                e.into()
            }
        }
    }
}

impl<T, E> Reply<T, E> for ServiceReply<T, E>
where
    T: serde::Serialize + 'static,
    E: From<Error> + Debug + std::error::Error + 'static,
{
}
