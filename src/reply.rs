use super::*;
use async::*;

/// Oneshot-style reply which contains response or error.
pub trait Reply<T, E>: serde::Serialize + From<Result<T, E>>
where
    T: serde::Serialize + 'static,
    E: From<Error> + 'static,
{
    /// write reply body
    fn reply(&self) -> HyperFuture {
        let encoded = match serde_json::to_vec(&self) {
            Ok(encoded) => encoded,
            Err(e) => {
                return Box::new(ok(resp_serv_err::<Error>(ErrorKind::EncodeJson(e).into())));
            }
        };
        let body: hyper::Body = encoded.into();
        let resp = hyper::server::Response::new()
            .with_status(hyper::StatusCode::Ok)
            .with_body(body);

        Box::new(ok(resp))
    }

    /// `state_serv_obj` build `HyperService` with given function `F` and state `S`.
    fn state_serv_obj<F, S, Req>(state: S, f: F) -> HyperService
    where
        Self: 'static,
        F: for<'a> Fn(&'a S, Req) -> Box<Future<Item = T, Error = E>> + 'static,
        S: 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = AsyncServiceFn::new(move |req| f(&state, req));
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `service_obj` builds `HyperService` with given function `F`.
    fn serv_obj<F, Req>(f: F) -> HyperService
    where
        Self: 'static,
        F: Fn(Req) -> Box<Future<Item = T, Error = E>> + 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = AsyncServiceFn::new(f);
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `state_serv_obj` builds `HyperService` with given function `F` and state `S`.
    fn state_serv_obj_sync<F, S, Req>(state: S, f: F) -> HyperService
    where
        Self: 'static,
        F: for<'a> Fn(&'a S, Req) -> Result<T, E> + 'static,
        S: 'static,
        Req: for<'de> serde::Deserialize<'de> + 'static,
    {
        let f = async::AsyncServiceFn::new(move |req| Box::new(result(f(&state, req))));
        Box::new(async::AsyncServiceStateW::<_, Self>::new(f))
    }

    /// `serv_obj` build `HyperService` with given function `F`.
    fn serv_obj_sync<F, Req>(f: F) -> HyperService
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
        msg: String,
        #[serde(skip)]
        _e: E,
    },
}
impl<T, E> From<Result<T, E>> for ServiceReply<T, E>
where
    T: serde::Serialize,
    E: Debug + Display,
{
    fn from(res: Result<T, E>) -> ServiceReply<T, E> {
        match res {
            Ok(resp) => ServiceReply::Ok { result: resp },
            Err(e) => {
                let reason = format!("{}", e);
                let msg = format!("{:?}", e);
                ServiceReply::Err { reason, msg, _e: e }
            }
        }
    }
}

impl<T, E> Reply<T, E> for ServiceReply<T, E>
where
    T: serde::Serialize + 'static,
    E: From<Error> + Debug + Display + 'static,
{
}
