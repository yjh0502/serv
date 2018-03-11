extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;

use hyper::server::{const_service, Http};

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
    let addr = ([127, 0, 0, 1], 3000).into();
    let service = const_service(serv::sync::serv(hello));

    let server = Http::new().bind(&addr, service).unwrap();
    eprintln!("listen: {}", server.local_addr().unwrap());
    server.run().unwrap();
}
