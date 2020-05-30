use actix::{Actor, Addr, SyncArbiter};
use failure::Fail;
use futures::future::{self, Future};
use futures::{stream::Stream, Sink};

use hyper::Response;
use hyper::{service::service_fn, Body, Method, Request, Server, StatusCode};

use lazy_static::lazy_static;

use serde_json::Value;
use std::io::{Error, ErrorKind};
use std::{fs, path::Path, thread};
use tokio::fs::File;
static INDEX: &[u8] = b"microservice Image";

mod actor;

lazy_static! {
    static ref DOWNLOAD_FILE: regex::Regex =
        regex::Regex::new("^/download/(?P<filename>\\w{20})?$").unwrap();
}

use self::actor::{
    counter::{CountActor, Counter},
    resize::ResizeActor,
};
use actor::resize::Resize;

#[derive(Clone)]
struct State {
    counter: Addr<CountActor>,
    resize: Addr<ResizeActor>,
}

fn main() {
    actix::run(|| {
        let resize = SyncArbiter::start(2, || ResizeActor);
        let counter = CountActor::new().start();
        let state = State { counter, resize };
        let addr = ([127, 0, 0, 1], 8080).into();
        let builder = Server::bind(&addr);

        let server = builder.serve(move || {
            let state = state.clone();
            service_fn(move |req| microservice(&state, req))
        });
        server.map_err(drop)
    });
}

fn microservice(
    state: &State,
    req: Request<Body>,
) -> Box<dyn Future<Item = Response<Body>, Error = Error> + Send> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            let future = count_up(state, req.uri().path()).map(|value| {
                println!("/: {}", value);
                Response::new(INDEX.into())
            });
            Box::new(future)
        }
        (&Method::POST, "/resize") => {
            let (width, height) = {
                let uri = req.uri().query().unwrap_or("");
                let query = queryst::parse(uri).unwrap_or(Value::Null);
                let w = to_number(&query["width"], 180);
                let h = to_number(&query["height"], 180);
                (w, h)
            };
            let resize = state.resize.clone();
            let body = req
                .into_body()
                .map_err(other)
                .concat2()
                .map(|chunk| chunk.to_vec())
                .and_then(move |buffer| {
                    let msg = Resize {
                        buffer,
                        width,
                        height,
                    };
                    resize
                        .send(msg)
                        .map_err(|err| other(err.compat()))
                        .and_then(|x| x.map_err(other))
                })
                .map(|resp| Response::new(resp.into()));
            let fut = count_up(state, "/resize").and_then(move |value| {
                println!("`/resize`: {:?}", value);
                body
            });
            Box::new(fut)
        }
        (&Method::GET, path) => {
            if let Some(cap) = DOWNLOAD_FILE.captures(path) {
                let uri = req.uri().query().unwrap_or("");
                let query = queryst::parse(uri).unwrap_or(Value::Null);
                let height = to_number(&query["height"], 100);
                let width = to_number(&query["width"], 200);
                let filename = cap.name("filename").unwrap().as_str();
                let mut directory_path = directory.to_path_buf();
                directory_path.push(filename);
                let open_file = File::open(directory_path);
                let body = open_file
                    .map(|file| {
                        let stream = FileChunkStream::new(file);
                        // let body = Body::wrap_stream(stream);
                        let st = stream.concat2().wait().unwrap();
                        let buffer = st.to_vec();
                        let task = future::lazy(move || convert(buffer, width, height));
                        let body = pool
                            .spawn(task)
                            .map_err(other)
                            .map(|resp| Response::new(resp.into()));

                        body
                    })
                    .wait()
                    .ok()
                    .unwrap();
                Box::new(body)
            } else {
                response_with_code(StatusCode::NOT_FOUND)
            }
        }
        _ => response_with_code(StatusCode::NOT_FOUND),
    }
}

fn count_up(state: &State, path: &str) -> impl Future<Item = u64, Error = Error> {
    let path = path.to_string();
    state
        .counter
        .send(Counter(path.clone()))
        .map_err(|err| other(err.compat()))
}

fn other<E>(err: E) -> Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    Error::new(ErrorKind::Other, err)
}

fn to_number(value: &Value, default: u16) -> u16 {
    value
        .as_str()
        .and_then(|x| x.parse::<u16>().ok())
        .unwrap_or(default)
}

// TODO: 这里是原来的handler
// fn handle_microservice(
//     req: Request<Body>,
//     directory: &Path,
//     pool: CpuPool,
// ) -> Box<dyn Future<Item = Response<Body>, Error = Error> + Send> {
//     match (req.method(), req.uri().path()) {
//         (&Method::GET, "/") => Box::new(future::ok(Response::new(INDEX.into()))),
//         (&Method::POST, "/upload") => {
//             let name: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

//             let mut directory_path = directory.to_path_buf();
//             directory_path.push(&name);
//             let create_file = File::create(directory_path);
//             let write = create_file.and_then(|file| {
//                 req.into_body().map_err(other).fold(file, |file, chunk| {
//                     tokio::io::write_all(file, chunk).map(|(file, _)| file)
//                 })
//             });
//             let body = write.map(|_| Response::new(name.into()));
//             Box::new(body)
//         }
//         (&Method::POST, "/resize") => {
//             let (width, height) = {
//                 let uri = req.uri().query().unwrap_or("");
//                 let query = queryst::parse(uri).unwrap_or(Value::Null);
//                 let w = to_number(&query["width"], 180);
//                 let h = to_number(&query["height"], 180);
//                 (w, h)
//             };
//             let body = req
//                 .into_body()
//                 .map_err(other)
//                 .concat2()
//                 .map(|chunk| chunk.to_vec())
//                 .and_then(move |buffer| {
//                     let task = future::lazy(move || convert(buffer, 100, 200));
//                     pool.spawn(task).map_err(other)
//                 })
//                 .map(|resp| Response::new(resp.into()));

//             Box::new(body)
//         }
//         (&Method::GET, path) => {
//             if let Some(cap) = DOWNLOAD_FILE.captures(path) {
//                 let uri = req.uri().query().unwrap_or("");
//                 let query = queryst::parse(uri).unwrap_or(Value::Null);
//                 let height = to_number(&query["height"], 100);
//                 let width = to_number(&query["width"], 200);
//                 let filename = cap.name("filename").unwrap().as_str();
//                 let mut directory_path = directory.to_path_buf();
//                 directory_path.push(filename);
//                 let open_file = File::open(directory_path);
//                 let body = open_file
//                     .map(|file| {
//                         let stream = FileChunkStream::new(file);
//                         // let body = Body::wrap_stream(stream);
//                         let st = stream.concat2().wait().unwrap();
//                         let buffer = st.to_vec();
//                         let task = future::lazy(move || convert(buffer, width, height));
//                         let body = pool
//                             .spawn(task)
//                             .map_err(other)
//                             .map(|resp| Response::new(resp.into()));

//                         body
//                     })
//                     .wait()
//                     .ok()
//                     .unwrap();
//                 Box::new(body)
//             } else {
//                 response_with_code(StatusCode::NOT_FOUND)
//             }
//         }
//         _ => response_with_code(StatusCode::NOT_FOUND),
//     }
// }
fn response_with_code(
    status_code: StatusCode,
) -> Box<dyn Future<Item = Response<Body>, Error = Error> + Send> {
    let body = Response::builder()
        .status(status_code)
        .body(Body::empty())
        .unwrap();
    Box::new(future::ok(body))
}
