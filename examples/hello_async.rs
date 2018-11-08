extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio;
extern crate tokio_timer;

use std::time::*;

use hyper::rt::Future;
use tokio::runtime::current_thread::Runtime;

#[derive(Serialize)]
struct HelloResp {
    msg: String,
}

fn hello(_req: serv::Empty) -> Box<Future<Item = HelloResp, Error = serv::Error>> {
    let delay = Instant::now() + Duration::from_secs(1);
    let f = tokio_timer::Delay::new(delay).then(|_| {
        Ok(HelloResp {
            msg: "hello, world".to_owned(),
        })
    });
    Box::new(f)
}

fn main() {
    use serv::server::{Routes, Server};
    let addr = "http://0.0.0.0:3000"
        .parse()
        .expect("failed to parse address");

    let mut routes = Routes::new();
    routes.push(hyper::Method::GET, "/", serv::async::serv(hello));
    let server = Server::new(routes);

    let mut rt = Runtime::new().expect("failed to create runtime");
    rt.block_on(server.run(addr)).expect("error on runtime");
}
