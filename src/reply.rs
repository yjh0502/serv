use super::*;

/// Oneshot-style reply which contains response or error.
pub trait Reply<T>: serde::Serialize
where
    T: serde::Serialize,
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
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum ServiceReply<T: serde::Serialize> {
    #[serde(rename = "ok")]
    Ok { result: T },
    //TODO: reason
    #[serde(rename = "error")]
    Err { reason: String, msg: String },
}
impl<T, E> From<Result<T, E>> for ServiceReply<T>
where
    T: serde::Serialize,
    E: std::fmt::Display + std::fmt::Debug,
{
    fn from(res: Result<T, E>) -> ServiceReply<T> {
        match res {
            Ok(resp) => ServiceReply::Ok { result: resp },
            Err(e) => {
                let reason = format!("{}", e);
                let msg = format!("{:?}", e);
                ServiceReply::Err { reason, msg }
            }
        }
    }
}

impl<T> Reply<T> for ServiceReply<T>
where
    T: serde::Serialize,
{
}
