#[macro_use]
extern crate error_chain;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio;

use tokio::runtime::current_thread::Runtime;

mod error {
    use super::*;

    error_chain! {
        foreign_links {
            Serv(serv::Error);
        }

        errors {
            Overflow {
                display("overflow")
            }
        }
    }
}
use error::*;

#[derive(Deserialize)]
struct AddReq {
    a: i8,
    b: i8,
}
#[derive(Serialize)]
struct AddResp {
    result: i8,
}
fn add(req: AddReq) -> Result<AddResp> {
    match req.a.checked_add(req.b) {
        Some(result) => Ok(AddResp { result }),
        None => bail!(ErrorKind::Overflow),
    }
}

fn main() {
    use serv::server::{Routes, Server};
    let addr = "http://0.0.0.0:3000"
        .parse()
        .expect("failed to parse address");

    let mut routes = Routes::new();
    routes.push(hyper::Method::GET, "/", serv::sync::serv(add));
    let server = Server::new(routes);

    let mut rt = Runtime::new().expect("failed to create runtime");
    rt.block_on(server.run(addr)).expect("error on runtime");
}
