extern crate futures;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio_timer;

use std::time::*;

use hyper::server::{const_service, Http};
use futures::*;
use tokio_timer::*;

#[derive(Serialize)]
struct HelloResp {
    msg: String,
}
fn hello(timer: &Timer, _req: serv::Empty) -> Box<Future<Item = HelloResp, Error = serv::Error>> {
    let f = timer
        .sleep(Duration::from_secs(1))
        .map_err(|_e| "timer failed".into())
        .map(|_| HelloResp {
            msg: "hello, world".to_owned(),
        });
    Box::new(f)
}

fn main() {
    let addr = ([127, 0, 0, 1], 3000).into();
    let timer = Timer::default();
    let service = const_service(serv::async::serv_state(timer, hello));

    let server = Http::new().bind(&addr, service).unwrap();
    eprintln!("listen: {}", server.local_addr().unwrap());
    server.run().unwrap();
}
