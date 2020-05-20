use futures::future::{self, Future};
use futures::{
    stream::Stream,
    sync::{mpsc, oneshot},
    Sink,
};
use futures_cpupool::CpuPool;
use hyper::Response;
use hyper::{service::service_fn, Body, Method, Request, Server, StatusCode};
use hyper_staticfile::FileChunkStream;
use image::{FilterType, ImageResult};
use lazy_static::lazy_static;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde_json::Value;
use std::io::{Error, ErrorKind};
use std::{fs, path::Path, thread};
use tokio::fs::File;
static INDEX: &[u8] = b"microservice Image";
lazy_static! {
    static ref DOWNLOAD_FILE: regex::Regex =
        regex::Regex::new("^/download/(?P<filename>\\w{20})?$").unwrap();
}
fn main() {
    let directory = Path::new("./files");
    let pool = CpuPool::new(4);
    fs::create_dir(directory).ok();
    let addr = ([127, 0, 0, 1], 8080).into();
    let builder = Server::bind(&addr);
    let server = builder.serve(move || {
        let pool = pool.clone();
        service_fn(move |req| handle_microservice(req, &directory, pool.clone()))
    });
    let server = server.map_err(drop);
    hyper::rt::run(server);
}
type WorkerResponse = Result<Vec<u8>, Error>;
struct WorkerRequest {
    height: u16,
    width: u16,
    buffer: Vec<u8>,
    tx: oneshot::Sender<WorkerResponse>,
}
fn start_worker() -> mpsc::Sender<WorkerRequest> {
    let (tx, rx) = mpsc::channel::<WorkerRequest>(1);
    thread::spawn(move || {
        let requests = rx.wait();
        for req in requests {
            if let Ok(req) = req {
                let resp = convert(req.buffer, req.width, req.height).map_err(other);
                req.tx.send(resp).ok();
            }
        }
    });
    tx
}
fn convert(data: Vec<u8>, width: u16, height: u16) -> ImageResult<Vec<u8>> {
    let format = image::guess_format(&data)?;
    let img = image::load_from_memory(&data)?;
    let scaled = img.resize(width as u32, height as u32, FilterType::Lanczos3);

    let mut result = Vec::new();
    scaled.write_to(&mut result, format)?;
    Ok(result)
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

fn handle_microservice(
    req: Request<Body>,
    directory: &Path,
    pool: CpuPool,
) -> Box<dyn Future<Item = Response<Body>, Error = Error> + Send> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => Box::new(future::ok(Response::new(INDEX.into()))),
        (&Method::POST, "/upload") => {
            let name: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

            let mut directory_path = directory.to_path_buf();
            directory_path.push(&name);
            let create_file = File::create(directory_path);
            let write = create_file.and_then(|file| {
                req.into_body().map_err(other).fold(file, |file, chunk| {
                    tokio::io::write_all(file, chunk).map(|(file, _)| file)
                })
            });
            let body = write.map(|_| Response::new(name.into()));
            Box::new(body)
        }
        (&Method::POST, "/resize") => {
            let (width, height) = {
                let uri = req.uri().query().unwrap_or("");
                let query = queryst::parse(uri).unwrap_or(Value::Null);
                let w = to_number(&query["width"], 180);
                let h = to_number(&query["height"], 180);
                (w, h)
            };
            let body = req
                .into_body()
                .map_err(other)
                .concat2()
                .map(|chunk| chunk.to_vec())
                .and_then(move |buffer| {
                    let task = future::lazy(move || convert(buffer, 100, 200));
                    pool.spawn(task).map_err(other)
                })
                .map(|resp| Response::new(resp.into()));

            Box::new(body)
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
            // response_with_code(StatusCode::NOT_FOUND)
            } else {
                response_with_code(StatusCode::NOT_FOUND)
            }
        }
        _ => response_with_code(StatusCode::NOT_FOUND),
    }
}
fn response_with_code(
    status_code: StatusCode,
) -> Box<dyn Future<Item = Response<Body>, Error = Error> + Send> {
    let body = Response::builder()
        .status(status_code)
        .body(Body::empty())
        .unwrap();
    Box::new(future::ok(body))
}
// fn alt_udp_echo() -> Result<(), Error> {
//     let from = "0.0.0.0:1234".parse()?;
//     let socket = UdpSocket::bind(&from)?;
//     Ok(())
// }
// fn single() {
//     let (tx, rx) = oneshot::channel::<u8>();
//     let receiver = rx.map(|item| {
//         println!("{}", item);
//     });
//     let sender = tx.send(8);
//     let execute_all = future::join_all(vec![to_box(receiver), to_box(sender)]).map(drop);
//     tokio::run(execute_all);
// }
// fn multiple() {
//     let (tx, rx) = mpsc::channel::<u8>(8);
//     let receiver = rx.fold(0, |acc, cur| future::ok(acc + cur)).map(|res| {
//         println!("the result is {}", res);
//     });
//     let send1 = tx.clone().send(1);
//     let send2 = tx.clone().send(2);
//     let send3 = tx.clone().send(3);

//     let execute_all = future::join_all(vec![
//         to_box(receiver),
//         to_box(send1),
//         to_box(send2),
//         to_box(send3),
//     ])
//     .map(drop);
//     drop(tx);
//     tokio::run(execute_all);
// }

// fn to_box<T>(fut: T) -> Box<dyn Future<Item = (), Error = ()> + Send>
// where
//     T: IntoFuture,
//     T::Future: Send + 'static,
//     T::Item: 'static,
//     T::Error: 'static,
// {
//     let fut = fut.into_future().map(drop).map_err(drop);
//     Box::new(fut)
// }

// fn send_spawn() {
//     let (tx, rx) = mpsc::channel::<u8>(8);
//     let receiver = rx.fold(0, |acc, cur| {
//         println!("receive value: {}", cur);
//         future::ok(acc + cur)
//     }).map(drop);

//     let spawn = stream::iter_ok::<_, ()>(1u8..11u8).map(move |item| {
//         let fut = tx.clone().send(item).map(drop).map_err(drop);
//         tokio::spawn(fut)
//     }).collect();
//     let execute_all = future::join_all(vec![
//         to_box(spawn),
//         to_box(receiver),
//     ]).map(drop);
//     tokio::run(execute_all);
// }
