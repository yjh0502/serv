use std::marker::PhantomData;

use super::*;
use reply::Reply;

/// `serv_state` build `HyperService` with given function `F` and state `S`.
pub fn serv_state<F, S, Req, Resp, E>(state: S, f: F) -> HyperService
where
    F: for<'a> Fn(&'a S, Req) -> Box<Future<Item = Resp, Error = E>> + 'static,
    S: 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + Display + 'static,
{
    reply::ServiceReply::serv_state(state, f)
}

/// `service` builds `HyperService` with given function `F`.
pub fn serv<F, Req, Resp, E>(f: F) -> HyperService
where
    F: Fn(Req) -> Box<Future<Item = Resp, Error = E>> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + Display + 'static,
{
    reply::ServiceReply::serv(f)
}

/// `AsyncServiceFn` implements `AsyncService` for given `F`
pub(crate) struct AsyncServiceFn<F, Req, Resp, E>
where
    F: Fn(Req) -> Box<Future<Item = Resp, Error = E>>,
    Req: 'static,
    Resp: 'static,
{
    f: F,
    _req: PhantomData<Req>,
    _resp: PhantomData<Resp>,
}
impl<F, Req, Resp, E> AsyncServiceFn<F, Req, Resp, E>
where
    F: Fn(Req) -> Box<Future<Item = Resp, Error = E>>,
    Req: 'static,
    Resp: 'static,
{
    pub(crate) fn new(f: F) -> Self {
        Self {
            f,
            _req: Default::default(),
            _resp: Default::default(),
        }
    }
}
impl<F, Req, Resp, E> AsyncService for AsyncServiceFn<F, Req, Resp, E>
where
    F: Fn(Req) -> Box<Future<Item = Resp, Error = E>>,
    Req: 'static,
    Resp: 'static,
{
    type Req = Req;
    type Resp = Resp;
    type E = E;
    fn call(&self, req: Self::Req) -> Box<Future<Item = Self::Resp, Error = Self::E>> {
        let f = &self.f;
        f(req)
    }
}

/// Oneshot-style asynchronous service.
pub(crate) trait AsyncService {
    type Req;
    type Resp;
    type E;

    fn call(&self, req: Self::Req) -> Box<Future<Item = Self::Resp, Error = Self::E>>;
}

/// `AsyncServiceStateW` implementes `tokio_service::Service` for `AsyncService`
pub(crate) struct AsyncServiceStateW<T, Reply> {
    inner: SyncObj<T>,
    reply: PhantomData<Reply>,
}
impl<T, Reply> AsyncServiceStateW<T, Reply> {
    pub(crate) fn new(t: T) -> Self {
        Self {
            inner: SyncObj::new(t),
            reply: Default::default(),
        }
    }
}

impl<T, Req, Resp, E, Reply> Service for AsyncServiceStateW<T, Reply>
where
    T: AsyncService<Req = Req, Resp = Resp, E = E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + 'static,
    Reply: reply::Reply<Resp, E> + 'static,
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = HyperFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        let obj = self.inner.clone();
        let f = parse_req(req)
            .map_err(E::from)
            .and_then(move |req| T::call(&obj, req))
            .then(|resp| Reply::from(resp).reply());
        Box::new(f)
    }
}
