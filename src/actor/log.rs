use actix::{Actor, Context, Handler, Message};
use syslog::{Facility, Formatter3164, Logger, LoggerBackend};

pub struct LoggerActor {
    writer: Logger<LoggerBackend, Formatter3164>,
}

impl LoggerActor {
    pub fn new() -> Self {
        let formatter = Formatter3164 {
            facility: Facility::LOG_USER,
            hostname: None,
            process: "rust-microservice".into(),
            pid: 0,
        };
        let writer = syslog::unix(formatter).unwrap();
        Self { writer }
    }
}

impl Actor for LoggerActor {
    type Context = Context<Self>;
}

pub struct Log(pub String);

impl Message for Log {
    type Result = ();
}

impl Handler<Log> for LoggerActor {
    type Result = ();
    fn handle(&mut self, Log(mesg): Log, ctx: &mut Self::Context) -> Self::Result {
        self.writer.info(mesg).unwrap()
    }
}
