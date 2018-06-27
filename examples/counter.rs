extern crate futures;
extern crate hyper;
#[macro_use]
extern crate serde_derive;
extern crate serv;
extern crate tokio;

use hyper::rt::Future;
use std::sync::atomic::*;

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
    let addr = ([127, 0, 0, 1], 3000).into();

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

    let server = hyper::server::Server::bind(&addr)
        .serve(move || Ok::<_, hyper::Error>(server.clone()))
        .map_err(|e| eprintln!("failed to serve: {:?}", e));

    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("failed to create runtime");
    rt.block_on(server).expect("error on runtime");
}
