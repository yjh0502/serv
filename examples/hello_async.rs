extern crate futures;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio_timer;

use std::time::*;

use hyper::server::{const_service, Http};
use futures::*;

#[derive(Serialize)]
struct HelloResp {
    msg: String,
}
fn hello(_req: serv::Empty) -> Box<Future<Item = HelloResp, Error = serv::Error>> {
    let deadline = Instant::now() + Duration::from_secs(1);
    let f = tokio_timer::Delay::new(deadline)
        .map_err(|_e| "timer failed".into())
        .map(|_| HelloResp {
            msg: "hello, world".to_owned(),
        });
    Box::new(f)
}

fn main() {
    let addr = ([127, 0, 0, 1], 3000).into();
    let service = const_service(serv::async::serv(hello));

    let server = Http::new().bind(&addr, service).unwrap();
    eprintln!("listen: {}", server.local_addr().unwrap());
    server.run().unwrap();
}
