use serde;

use super::*;
use reply::Reply;

/// `serv_state` builds `HyperService` with given function `F` and state `S`.
pub fn serv_state<F, S, Req, Resp, E>(state: S, f: F) -> HyperService
where
    F: for<'a> Fn(&'a S, Req) -> Result<Resp, E> + 'static,
    S: 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + std::error::Error + 'static,
{
    reply::ServiceReply::serv_state_sync(state, f)
}

/// `serv` build `HyperService` with given function `F`.
pub fn serv<F, Req, Resp, E>(f: F) -> HyperService
where
    F: Fn(Req) -> Result<Resp, E> + 'static,
    Req: for<'de> serde::Deserialize<'de> + 'static,
    Resp: serde::Serialize + 'static,
    E: From<Error> + Debug + std::error::Error + 'static,
{
    reply::ServiceReply::serv_sync(f)
}
