use std::sync;
use std::thread;
use std::time;
use std::vec;

use crate::builder;
use futures::executor;
use futures::future;

// TODO: check usage of &str + lifetime (won't work as listen/ping running thread require 'static lifetime) vs String.
// TODO: look into caching connections, etc.
// TODO: look into agents advertising capabilities - OCCI style of course :-)

/// Type of messages that can be passed between agents.
pub enum Msg {
    Ping(String),
    Message(String),
    Kill(),
}

impl Msg {
    /// Convert a message - ensures format as the listener expects it.
    pub fn to_msg(&self) -> String {
        match &self {
            Msg::Ping(content) => String::from("P@") + content,
            Msg::Message(content) => String::from("M@") + content,
            Msg::Kill() => String::from("K@0"),
        }
    }
}

///
/// An agent in a multi-agent system-of-systems enabling comms.
///
pub trait Agent {
    /// Return the current messages for this Agent.
    fn retrieve(&self) -> Vec<String>;
    /// Broadcast a message to the participants in the system-of-systems.
    fn broadcast(&self, msg: &str);
}

///
/// An Agent using ZeroMQ to communicate with it's peers.
///
pub struct ZeroAgent {
    pub ep: String,
    pub peers: sync::Arc<sync::Mutex<vec::Vec<String>>>,
    pub msgs: sync::Arc<sync::Mutex<vec::Vec<String>>>,
    pub ctxt: zmq::Context,
    pub wait: u64,
    pub timeout: u64,
}

///
/// Very simple implementation of an agent in a multi-agent system. Agents can join and leave the
/// system on the fly. No centralized component is needed - probably also not the nicest solution
/// available, but does the trick for now.
///
/// Using ZeroMQ for comms - using REP/REQ sockets atm; Could be done more elegant with other
/// socket type - but works for now - KISS :-)
///
impl ZeroAgent {
    /// Returns a builder for the ZeroAgent.
    pub fn builder(ep: String) -> builder::AgentBuilder {
        builder::AgentBuilder::new(ep)
    }

    /// add a peer to the multi-agent system.
    pub fn add_peer(&self, ep: String) {
        let rcp: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.peers);
        let mut peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
        if (!peers.contains(&ep)) && ep != self.ep {
            peers.push(ep);
        }
        drop(peers);
    }

    /// send a message to a particular peer.
    pub fn send_msg(&self, peer: &str, msg: &Msg) {
        let client = &self.ctxt.socket(zmq::REQ).unwrap();
        client.connect(peer).expect("Could not connect to peer");
        client.send(msg.to_msg().as_str(), 0).unwrap();
        client.recv_msg(0).unwrap(); // Wait for ack...
    }

    /// Activate the agents - will start listener and mgmt. threads.
    pub fn activate(&self) -> (thread::JoinHandle<()>, thread::JoinHandle<()>) {
        // The listener threads, watches for incoming messages.
        let rcp_0: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.peers);
        let msgs: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.msgs);
        let ep_0: String = self.ep.clone();
        let ctxt_0: zmq::Context = self.ctxt.clone();
        let list_th = thread::spawn(move || {
            listen(ctxt_0, ep_0, rcp_0, msgs);
        });

        // Ping thread - assures reasonably consistency.
        let rcp_1: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.peers);
        let ep_1: String = self.ep.clone();
        let ctxt_1: zmq::Context = self.ctxt.clone();
        let timeout_1 = self.timeout;
        let wait_1 = self.wait;
        let ping_th = thread::spawn(move || {
            ping(ctxt_1, ep_1, rcp_1, wait_1, timeout_1);
        });
        (list_th, ping_th)
    }

    pub fn get_n_peers(&self) -> usize {
        let rcp: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.peers);
        let peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
        let n_peers: usize = peers.len();
        drop(peers);
        n_peers
    }
}

/// Listen to incoming messages and act accordingly.
fn listen(
    ctxt: zmq::Context,
    ep: String,
    rcp: sync::Arc<sync::Mutex<Vec<String>>>,
    msg_rcp: sync::Arc<sync::Mutex<Vec<String>>>,
) {
    let mut done: bool = false;
    let list: zmq::Socket = ctxt.socket(zmq::REP).unwrap();
    list.bind(&ep).expect("Could not bind...");

    while !done {
        let msg: zmq::Message = list.recv_msg(0).unwrap();
        let tmp: String = msg.as_str().unwrap().to_string();
        list.send("0", 0).unwrap();

        let split = tmp.split('@');
        let data = split.collect::<Vec<&str>>();

        if data[0] == "P" {
            let mut peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
            for peer in data[1].to_string().split(',') {
                if !peers.contains(&peer.to_string()) {
                    peers.push(peer.to_string());
                }
            }
            drop(peers);
        } else if data[0] == "M" {
            let mut msgs: sync::MutexGuard<Vec<String>> = msg_rcp.lock().unwrap();
            msgs.push(String::from(data[1]));
            drop(msgs);
        } else if data[0] == "K" {
            let mut peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
            peers.clear();
            drop(peers);
            done = true;
        }
    }
}

///
/// Ping an individual peer; will return empty string if all is good, otherwise the URI.
///
async fn ping_peer(
    ctxt: &zmq::Context,
    peer: &str,
    msg: &Msg,
    wait: u64,
) -> Result<String, &'static str> {
    let mut res: String = "".to_string();

    let client: zmq::Socket = ctxt.socket(zmq::REQ).unwrap();
    client.set_connect_timeout(2).unwrap();
    // TODO: would be great to set: ZMQ_REQ_CORRELATE; not support atm.
    client.connect(peer).expect("Could not connect to peer");
    client.send(msg.to_msg().as_str(), 0).unwrap();
    thread::sleep(time::Duration::from_millis(wait));
    if client.recv_msg(zmq::DONTWAIT).is_err() {
        res = peer.to_string();
    }
    client.disconnect(peer).unwrap();
    Ok(res)
}

///
/// Will on a given timout try to ping the host it knows and if needed remove peers from the list
/// of known neighbours.
///
/// Could be optimized by only sending delta in data between last msg and new one.
///
fn ping(
    ctxt: zmq::Context,
    my_ep: String,
    rcp: sync::Arc<sync::Mutex<Vec<String>>>,
    wait: u64,
    timeout: u64,
) {
    loop {
        let mut peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
        let joined = peers.join(",");
        let msg = Msg::Ping(joined);

        let fut_values: _ = async {
            let mut futures: Vec<_> = vec![];
            for peer in peers.iter() {
                if peer != &my_ep {
                    futures.push(ping_peer(&ctxt, peer, &msg, wait));
                }
            }
            let res: Vec<String> = future::try_join_all(futures).await.unwrap();
            res
        };
        let dead_peers: Vec<String> = executor::block_on(fut_values);
        peers.retain(|x: &String| !dead_peers.contains(x));

        if peers.is_empty() {
            break;
        }

        drop(peers);
        thread::sleep(time::Duration::from_secs(timeout));
    }
}

impl Agent for ZeroAgent {
    fn retrieve(&self) -> Vec<String> {
        let mut msgs: sync::MutexGuard<Vec<String>> = self.msgs.lock().unwrap();
        let res = msgs.clone();
        msgs.clear();
        drop(msgs);
        res
    }
    fn broadcast(&self, msg: &str) {
        let rcp: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::clone(&self.peers);
        let peers: sync::MutexGuard<Vec<String>> = rcp.lock().unwrap();
        for peer in peers.iter() {
            if peer != &self.ep {
                self.send_msg(peer, &Msg::Message(msg.to_string()));
            }
        }
        drop(peers);
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time;

    use crate::agent;
    // Need to bring this in scope so I can use retrieve().
    use crate::agent::Agent;

    fn send_kill(ep: &str) {
        let ctxt = zmq::Context::new();
        let client = ctxt.socket(zmq::REQ).unwrap();
        client.connect(&ep).expect("Could not connect to peer");
        client
            .send(agent::Msg::Kill().to_msg().as_str(), 0)
            .unwrap();
        client.recv_msg(0).unwrap();
        client.disconnect(&ep).unwrap();
    }

    // Test for success.

    #[test]
    fn test_builder_for_success() {
        agent::ZeroAgent::builder("inproc://#0".to_string()).build();
    }

    #[test]
    fn test_add_peer_for_success() {
        let a_0 = agent::ZeroAgent::builder("inproc://#1".to_string()).build();
        a_0.add_peer("inproc://#1".to_string());
    }

    #[test]
    fn test_send_msg_for_success() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:8787".to_string()).build();
        let th0 = a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:8989".to_string()).build();
        let th1 = a_1.activate();
        a_1.add_peer("tcp://127.0.0.1:8787".to_string());

        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        a_1.send_msg(
            "tcp://127.0.0.1:8787",
            &agent::Msg::Message(String::from("hello")),
        );

        send_kill("tcp://127.0.0.1:8787");
        send_kill("tcp://127.0.0.1:8989");
        th0.0.join().unwrap();
        th0.1.join().unwrap();
        th1.0.join().unwrap();
        th1.1.join().unwrap();
    }

    #[test]
    fn test_activate_for_success() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:1234".to_string()).build();
        let ths = a_0.activate();
        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        send_kill("tcp://127.0.0.1:1234");
        ths.0.join().unwrap();
        ths.1.join().unwrap();
    }

    #[test]
    fn test_get_n_peers_for_success() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:2345".to_string()).build();
        let ths = a_0.activate();
        a_0.get_n_peers();
        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        send_kill("tcp://127.0.0.1:2345");
        ths.0.join().unwrap();
        ths.1.join().unwrap();
    }

    #[test]
    fn test_retrieve_for_success() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:9898".to_string()).build();
        let ths = a_0.activate();
        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        a_0.retrieve();
        send_kill("tcp://127.0.0.1:9898");
        ths.0.join().unwrap();
        ths.1.join().unwrap();
    }

    #[test]
    fn test_broadcast_for_success() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:3456".to_string()).build();
        let th0 = a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:3457".to_string()).build();
        let th1 = a_1.activate();
        a_1.add_peer("tcp://127.0.0.1:3456".to_string());

        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        a_1.broadcast("hello");

        send_kill("tcp://127.0.0.1:3456");
        send_kill("tcp://127.0.0.1:3457");
        th0.0.join().unwrap();
        th0.1.join().unwrap();
        th1.0.join().unwrap();
        th1.1.join().unwrap();
    }

    // Test for failure.

    // TODO: figure this out...

    // Test for sanity.

    #[test]
    fn test_send_msg_for_sanity() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:5000".to_string()).build();
        a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:5001".to_string()).build();
        a_1.add_peer(String::from("tcp://127.0.0.1:5000"));
        a_1.activate();

        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        a_0.send_msg(
            "tcp://127.0.0.1:5001",
            &agent::Msg::Message(String::from("Hello")),
        );

        // a_0 was sender so msg list is empty...
        assert!(a_0.msgs.lock().unwrap().to_vec().is_empty());
        // a_1 should have received a hello...
        assert_eq!(a_1.msgs.lock().unwrap().to_vec(), vec!["Hello"]);

        send_kill("tcp://127.0.0.1:5000");
        send_kill("tcp://127.0.0.1:5001");
    }

    #[test]
    fn test_activate_for_sanity() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:5002".to_string()).build();
        a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:5003".to_string()).build();
        a_1.add_peer("tcp://127.0.0.1:5002".to_string());
        a_1.activate();

        thread::sleep(time::Duration::from_millis(2 * a_0.wait));
        send_kill("tcp://127.0.0.1:5003");
        thread::sleep(time::Duration::from_secs(2 * a_0.timeout));
        // When a_1 is gone, a_0 should only know itself...
        assert_eq!(
            a_0.peers.lock().unwrap().to_vec(),
            vec!["tcp://127.0.0.1:5002"]
        );
        send_kill("tcp://127.0.0.1:5002");
    }

    #[test]
    fn test_get_n_peers_for_sanity() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:5004".to_string()).build();
        a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:5005".to_string()).build();
        a_1.add_peer("tcp://127.0.0.1:5004".to_string());
        a_1.activate();

        // slightly longer then the timeout of peers pinging each other...
        thread::sleep(time::Duration::from_secs(2 * a_0.timeout));
        // both should know about each other:
        assert_eq!(a_0.get_n_peers(), 2);
        assert_eq!(a_1.get_n_peers(), 2);

        send_kill("tcp://127.0.0.1:5004");
        send_kill("tcp://127.0.0.1:5005");
    }

    #[test]
    fn test_retrieve_for_sanity() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:5006".to_string()).build();
        a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:5007".to_string()).build();
        a_1.add_peer(String::from("tcp://127.0.0.1:5006"));
        a_1.activate();

        thread::sleep(time::Duration::from_secs(1.2 as u64 * a_0.timeout));
        a_0.send_msg(
            "tcp://127.0.0.1:5007",
            &agent::Msg::Message(String::from("Foo")),
        );

        let msgs = a_1.retrieve();
        assert_eq!(msgs, vec!["Foo"]);

        send_kill("tcp://127.0.0.1:5006");
        send_kill("tcp://127.0.0.1:5007");
    }

    #[test]
    fn test_broadcast_for_sanity() {
        let a_0 = agent::ZeroAgent::builder("tcp://127.0.0.1:5008".to_string()).build();
        a_0.activate();
        let a_1 = agent::ZeroAgent::builder("tcp://127.0.0.1:5009".to_string()).build();
        a_1.add_peer(String::from("tcp://127.0.0.1:5008"));
        a_1.activate();

        thread::sleep(time::Duration::from_secs(1.2 as u64 * a_0.timeout));
        a_0.broadcast("bar");

        let msgs = a_0.retrieve();
        assert_eq!(msgs.len(), 0); // should not talk to itself.
        let msgs = a_1.retrieve();
        assert_eq!(msgs, vec!["bar"]); // other agent should know...

        send_kill("tcp://127.0.0.1:5008");
        send_kill("tcp://127.0.0.1:5009");
    }
}
