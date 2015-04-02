use std::thread;
use std::sync::mpsc;
use websocket::{Server, Message, Sender, Receiver};
use websocket::server::sender;
use websocket::stream::WebSocketStream;
use std::collections::{HashMap, BitSet};
use std::io::prelude::*;
use std::num::ToPrimitive;

use value::Value;
use index;
use flow::{Changes, FlowState, Flow};
use compiler::{compile, World};
use rustc_serialize::json::{Json, ToJson};

trait FromJson {
    fn from_json(json: &Json) -> Self;
}

impl ToJson for Value {
    fn to_json(&self) -> Json {
        match *self {
            Value::String(ref string) => Json::String(string.clone()),
            Value::Float(float) => Json::F64(float),
            Value::Tuple(ref tuple) => tuple.to_json(),
            Value::Relation(_) => panic!("No json encoding for relations"),
        }
    }
}

impl FromJson for Value {
    fn from_json(json: &Json) -> Self {
        match *json {
            Json::String(ref string) => Value::String(string.clone()),
            Json::F64(float) => Value::Float(float),
            Json::I64(int) => Value::Float(int.to_f64().unwrap()),
            Json::U64(uint) => Value::Float(uint.to_f64().unwrap()),
            Json::Array(ref array) => Value::Tuple(array.iter().map(|j| Value::from_json(j)).collect()),
            _ => panic!("Cannot decode {:?} as Value", json),
        }
    }
}

impl<T: FromJson> FromJson for Vec<T> {
    fn from_json(json: &Json) -> Self {
        json.as_array().unwrap().iter().map(FromJson::from_json).collect()
    }
}

pub struct Event {
    changes: Changes,
}

impl ToJson for Event {
    fn to_json(&self) -> Json {
        Json::Object(vec![
            ("changes".to_string(), Json::Object(
                self.changes.iter().map(|&(ref view_id, ref view_changes)| {
                    (view_id.to_string(), Json::Object(vec![
                        ("inserted".to_string(), view_changes.inserted.to_json()),
                        ("removed".to_string(), view_changes.removed.to_json()),
                        ].into_iter().collect()))
                }).collect()))].into_iter().collect())
    }
}

impl FromJson for Event {
    fn from_json(json: &Json) -> Self {
        Event{
            changes: json.as_object().unwrap()["changes"]
            .as_object().unwrap().iter().map(|(view_id, view_changes)| {
                (view_id.to_string(), index::Changes{
                    inserted: FromJson::from_json(&view_changes.as_object().unwrap()["inserted"]),
                    removed: FromJson::from_json(&view_changes.as_object().unwrap()["removed"]),
                })
            }).collect()
        }
    }
}

struct Instance {
    input: World,
    flow: Flow,
    output: FlowState,
}

impl Instance {
    pub fn change(&mut self, changes: Changes) -> Changes {
        self.input.change(changes);
        let mut input_clone = self.input.clone();
        let (flow, mut output) = compile(&mut input_clone);
        flow.run(&mut output);
        let changes = flow.changes_since(&output, &self.flow, &self.output);
        self.flow = flow;
        self.output = output;
        changes
    }
}

// TODO holy crap why is everything blocking? this is a mess
pub fn serve() -> (mpsc::Receiver<Event>, mpsc::Receiver<sender::Sender<WebSocketStream>>) {
    let (input_sender, input_receiver) = mpsc::channel();
    let (sender_sender, sender_receiver) = mpsc::channel();
    thread::spawn(move || {
        let server = Server::bind("127.0.0.1:2794").unwrap();
        for connection in server {
            let input_sender = input_sender.clone();
            let sender_sender = sender_sender.clone();
            thread::spawn(move || {
                // accept request
                let request = connection.unwrap().read_request().unwrap();
                request.validate().unwrap();
                let response = request.accept();
                let (mut sender, mut receiver) = response.send().unwrap().split();

                let ip = sender.get_mut().peer_addr().unwrap();
                println!("Connection from {}", ip);
                ::std::io::stdout().flush().unwrap(); // TODO is this actually necessary?

                // hand over sender
                sender_sender.send(sender).unwrap();

                // handle messages
                for message in receiver.incoming_messages() {
                    let message = message.unwrap();
                    match message {
                        Message::Text(text) => {
                            let json = Json::from_str(&text).unwrap();
                            let event = FromJson::from_json(&json);
                            input_sender.send(event).unwrap();
                        }
                        _ => panic!("Unknown message: {:?}", message)
                    }
                }
            });
        }
    });
    (input_receiver, sender_receiver)
}

pub fn run() {
    let empty_world = World{views: HashMap::new()};
    let empty_flow = Flow{nodes: Vec::new()};
    let empty_output = FlowState{outputs: Vec::new(), dirty: BitSet::new()};
    let mut instance = Instance{
        input: empty_world,
        flow: empty_flow.clone(),
        output: empty_output.clone(),
    };
    let mut senders: Vec<sender::Sender<_>> = Vec::new();
    let (input_receiver, sender_receiver) = serve();
    loop {
        select!(
            input = input_receiver.recv() => {
                let input = input.unwrap();
                let changes = instance.change(input.changes);
                let text = format!("{}", Event{changes: changes}.to_json());
                for sender in senders.iter_mut() {
                    sender.send_message(Message::Text(text.clone())).unwrap();
                }
            },
            sender = sender_receiver.recv() => {
                let mut sender = sender.unwrap();
                let changes = instance.flow.changes_since(&instance.output, &empty_flow, &empty_output);
                let text = format!("{}", Event{changes: changes}.to_json());
                sender.send_message(Message::Text(text)).unwrap();
                senders.push(sender)
            }
            )
    }
}