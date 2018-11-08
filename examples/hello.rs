extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio;

use tokio::runtime::current_thread::Runtime;

#[derive(Serialize)]
struct HelloResp {
    msg: String,
}
fn hello(_req: serv::Empty) -> serv::error::Result<HelloResp> {
    Ok(HelloResp {
        msg: "hello, world".to_owned(),
    })
}

fn main() {
    use serv::server::{Routes, Server};

    let addr = "http://0.0.0.0:3000"
        .parse()
        .expect("failed to parse address");
    let mut routes = Routes::new();
    routes.push(hyper::Method::GET, "/", serv::sync::serv(hello));
    let server = Server::new(routes);

    let mut rt = Runtime::new().expect("failed to create runtime");
    rt.block_on(server.run(addr)).expect("error on runtime");
}
