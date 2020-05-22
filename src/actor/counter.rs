use actix::Actor;
use actix::Context;
use actix::Handler;
use actix::Message;
use std::collections::HashMap;

type Value = u64;
pub struct CountActor {
    counter: HashMap<String, Value>,
}

impl CountActor {
    pub fn new() -> Self {
        Self {
            counter: HashMap::new(),
        }
    }
}

impl Actor for CountActor {
    type Context = Context<Self>;
}

pub struct Counter(pub String);

impl Message for Counter {
    type Result = Value;
}

impl Handler<Counter> for CountActor {
    type Result = Value;
    fn handle(&mut self, Counter(path): Counter, ctx: &mut Self::Context) -> Self::Result {
       let value = self.counter.entry(path).or_default();
       *value +=1 ;
       *value
    }
}
