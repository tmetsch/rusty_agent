use std::env;
use std::thread;
use std::time;

use rusty_agent::agent;
use rusty_agent::agent::Agent;

const TIMEOUT: u64 = 750;

/// Starts an agent of a multi-agent system.
fn main() {
    // let's parse the id for this agent.
    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        panic!("Usage: cargo run --example sample_agent <id>");
    }
    let agent_id_str: &String = &args[1];
    let ep = "tcp://127.0.0.1:800".to_owned() + agent_id_str;
    let agent = agent::ZeroAgent::builder(ep.clone()).build();
    let threads = agent.activate();

    // If I'm the the first agent broadcast a msg - else connect myself to the one started before me...
    let agent_id: u32 = agent_id_str.parse::<u32>().expect("Provide Id as u32...");
    if agent_id == 0 {
        // Assure we have a partner ready...
        let mut i: i32 = 0;
        let mut ready = false;
        while i < 10 {
            if agent.get_n_peers() > 1 {
                ready = true;
            }
            thread::sleep(time::Duration::from_millis(TIMEOUT));
            i += 1;
        }
        if !ready {
            panic!("Could not find peers!");
        }
        // send a hello world to all known neighbours.
        agent.broadcast("hello world.");
        println!("Send broadcast to {} neighbour(s).", agent.get_n_peers())
    } else {
        let ngbh_id = agent_id - 1;
        let other_ep = "tcp://127.0.0.1:800".to_owned() + &ngbh_id.to_string();
        agent.add_peer(other_ep);

        // now wait for messages
        let mut j: i32 = 0;
        while j < 10 {
            let msgs = agent.retrieve();
            println!("{} - message received: {:?}.", j, msgs);
            if msgs.len() > 0 {
                break;
            }
            thread::sleep(time::Duration::from_millis(TIMEOUT));
            j += 1
        }
    }

    // cleanup time...
    agent.send_msg(&ep, &agent::Msg::Kill());
    threads.1.join().expect("waiting");
    threads.0.join().expect("waiting");
}
