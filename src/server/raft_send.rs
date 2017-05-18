// Copyright 2017 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt;
use std::sync::Arc;
use std::net::SocketAddr;

use futures::sync::mpsc::{self, UnboundedSender};
use futures::{Future, Sink, Stream};
use tokio_core::reactor::Handle;
use grpc::{Environment, ChannelBuilder};
use kvproto::raft_serverpb::RaftMessage;
use kvproto::tikvpb_grpc::TikvClient;

use util::collections::HashMap;
use util::worker::FutureRunnable;
use super::{Error, Result};

/// `SendTask` delivers a raft message to other store.
pub struct SendTask {
    pub addr: SocketAddr,
    pub msg: RaftMessage,
}

impl fmt::Display for SendTask {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "send raft message to {:?}", self.addr)
    }
}

struct Conn {
    _client: TikvClient,
    stream: UnboundedSender<RaftMessage>,
}

impl Conn {
    fn new(env: Arc<Environment>, addr: SocketAddr, handle: &Handle) -> Conn {
	error!("hehe new conn --> {}", addr);
        let channel = ChannelBuilder::new(env).connect(&format!("{}", addr));
        let client = TikvClient::new(channel);
        let (tx, rx) = mpsc::unbounded();
        handle.spawn(client.raft()
            .sink_map_err(Error::from)
            .send_all(rx.map(|x: RaftMessage| {
               error!("hehe stream recv, to: {:?}", x.get_to_peer());
               x
	    }).map_err(|_| Error::Sink))
            .map(|_| ())
            .map_err(move |e| error!("hehe-->{} stream error: {:?}", addr, e)));
        Conn {
            _client: client,
            stream: tx,
        }
    }
}

/// `SendRunner` is used for sending raft messages to other stores.
pub struct SendRunner {
    env: Arc<Environment>,
    conns: HashMap<SocketAddr, Conn>,
}

impl SendRunner {
    pub fn new(env: Arc<Environment>) -> SendRunner {
        SendRunner {
            env: env,
            conns: HashMap::default(),
        }
    }

    fn get_conn(&mut self, addr: SocketAddr, handle: &Handle) -> &Conn {
        let env = self.env.clone();
        self.conns
            .entry(addr)
            .or_insert_with(|| Conn::new(env, addr, handle))
    }

    fn send(&mut self, t: SendTask, handle: &Handle) -> Result<()> {
        let conn = self.get_conn(t.addr, handle);
        box_try!(UnboundedSender::send(&conn.stream, t.msg));
	error!("hehe--> {} ok.", t.addr);
        Ok(())
    }
}

impl FutureRunnable<SendTask> for SendRunner {
    fn run(&mut self, t: SendTask, handle: &Handle) {
        let addr = t.addr;
        if let Err(_) = self.send(t, handle) {
	    error!("hehe-->{} fail, remove conn.", addr);
            self.conns.remove(&addr);
        }
    }
}
