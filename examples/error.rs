#[macro_use]
extern crate error_chain;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;

use hyper::server::{const_service, Http};
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
    let addr = ([127, 0, 0, 1], 3000).into();
    let service = const_service(serv::sync::serv(add));

    let server = Http::new().bind(&addr, service).unwrap();
    eprintln!("listen: {}", server.local_addr().unwrap());
    server.run().unwrap();
}
