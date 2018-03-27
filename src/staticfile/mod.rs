extern crate futures;
extern crate hyper;
extern crate tokio_core;
extern crate url;

mod requested_path;
mod static_service;

pub use self::static_service::Static;
