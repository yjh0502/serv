use std;
use std::marker::PhantomData;

use serde;
use hyper;
use futures::*;
use futures::future::*;
use hyper::server::{Request, Response, Service};

use super::*;

//TODO: Arc? Rc?
type SyncObj<T> = std::rc::Rc<T>;

pub fn state_serv_obj<F, S, Req, Resp, E>(state: S, f: F) -> HyperService
where
    F: for<'a> Fn(&'a S, Req) -> Result<Resp, E> + 'static,
    S: 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
{
    let f = SyncServiceFn {
        f: move |req| f(&state, req),
        _req: Default::default(),
        _resp: Default::default(),
    };
    Box::new(SyncServiceStateW(SyncObj::new(f)))
}

pub fn serv_obj<F, Req, Resp, E>(f: F) -> HyperService
where
    F: Fn(Req) -> Result<Resp, E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
{
    let f = SyncServiceFn {
        f: f,
        _req: Default::default(),
        _resp: Default::default(),
    };
    Box::new(SyncServiceStateW(SyncObj::new(f)))
}

pub fn with_service_fn<F, Req, Resp, E>(f: F, req: Request) -> HyperFuture
where
    F: Fn(Req) -> Result<Resp, E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
{
    let f = SyncServiceFn {
        f: f,
        _req: Default::default(),
        _resp: Default::default(),
    };
    let s = SyncServiceFnW(f);
    with_service(s, req)
}

pub fn with_service<S, Req, Resp, E>(s: S, req: Request) -> HyperFuture
where
    S: Service<Request = Req, Response = Resp, Error = E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: std::fmt::Display + std::fmt::Debug,
{
    let f = parse_req(req)
        .and_then(move |req| s.call(req).then(|res| ok(ServiceResp::from(res))))
        .or_else(|e| ok(ServiceResp::from(Err(e))))
        .and_then(reply);
    Box::new(f)
}

trait SyncService {
    type Req;
    type Resp;
    type E;

    fn call(&self, req: Self::Req) -> Result<Self::Resp, Self::E>;
}

struct SyncServiceFn<F, Req, Resp, E>
where
    F: Fn(Req) -> Result<Resp, E>,
    Req: 'static,
    Resp: 'static,
{
    f: F,
    _req: PhantomData<Req>,
    _resp: PhantomData<Resp>,
}

impl<F, Req, Resp, E> SyncService for SyncServiceFn<F, Req, Resp, E>
where
    F: Fn(Req) -> Result<Resp, E>,
    Req: 'static,
    Resp: 'static,
{
    type Req = Req;
    type Resp = Resp;
    type E = E;
    fn call(&self, req: Req) -> Result<Resp, E> {
        let f = &self.f;
        f(req)
    }
}

struct SyncServiceFnW<T>(T);

impl<T, Req, Resp, E> Service for SyncServiceFnW<T>
where
    T: SyncService<Req = Req, Resp = Resp, E = E>,
    Req: 'static,
    Resp: 'static,
    E: 'static,
{
    type Request = T::Req;
    type Response = T::Resp;
    type Error = T::E;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        Box::new(result(T::call(&self.0, req)))
    }
}

struct SyncServiceStateW<T>(SyncObj<T>);

impl<T, Req, Resp, E> Service for SyncServiceStateW<T>
where
    T: SyncService<Req = Req, Resp = Resp, E = E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = HyperFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        let obj = self.0.clone();
        let f = parse_req(req)
            .and_then(move |req| ok(ServiceResp::from(T::call(&obj, req))))
            .or_else(|e| ok(ServiceResp::from(Err(e))))
            .and_then(reply);
        Box::new(f)
    }
}
