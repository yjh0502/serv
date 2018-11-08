extern crate futures;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio;

use std::sync::atomic::*;
use tokio::runtime::current_thread::Runtime;

struct State {
    counter: AtomicUsize,
}

#[derive(Serialize)]
struct CounterResp {
    counter: usize,
}
fn counter(s: &State, _req: serv::Empty) -> serv::error::Result<CounterResp> {
    let counter = s.counter.fetch_add(1, Ordering::SeqCst);
    Ok(CounterResp { counter })
}

fn main() {
    use serv::server::{Routes, Server};
    let addr = "http://0.0.0.0:3000"
        .parse()
        .expect("failed to parse address");

    let state = State {
        counter: Default::default(),
    };

    let mut routes = Routes::new();
    routes.push(
        hyper::Method::GET,
        "/",
        serv::sync::serv_state(state, counter),
    );
    let server = Server::new(routes);

    let mut rt = Runtime::new().expect("failed to create runtime");
    rt.block_on(server.run(addr)).expect("error on runtime");
}
