use serde;

use super::*;
use reply::Reply;

/// `state_serv_obj` builds `HyperService` with given function `F` and state `S`.
pub fn state_serv_obj<F, S, Req, Resp, E>(state: S, f: F) -> HyperService
where
    F: for<'a> Fn(&'a S, Req) -> Result<Resp, E> + 'static,
    S: 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + Display + 'static,
{
    reply::ServiceReply::state_serv_obj_sync(state, f)
}

/// `serv_obj` build `HyperService` with given function `F`.
pub fn serv_obj<F, Req, Resp, E>(f: F) -> HyperService
where
    F: Fn(Req) -> Result<Resp, E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + Display + 'static,
{
    reply::ServiceReply::serv_obj_sync(f)
}
