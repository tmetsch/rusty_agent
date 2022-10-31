
# Multi-agent framework in Rust

This is a simple framework enabling [multi-agent systems](https://en.wikipedia.org/wiki/Multi-agent_system). Originally
developed to test distributed planning & decision-making for [this](https://github.com/tmetsch/rusty_planner) crate, it
can be used for various other use cases.

It uses [ZeroMQ](https://zeromq.org/) for messaging between the agents. Each agent internally works with two threads. 
One which listen to message from other agents, another which continuously "pings" it's neighbours. This pinging allows 
agents to join and leave a system-of-systems on the fly. Once an agent is pinged it will include information about the 
neighbours it knows about to the other agents.

## Example 

An example is provided - first start the first agent, it will broadcast a hello world to its neighbours. It will wait 
till other agents have joined the system-of-systems:

```bash
$ cargo run --example sample_agent 0
```

The program parameter "0" indicates the id of the agent, as well as the port it will use.

Now another agent can be started - which will connect to the previous one, wait for the "hello world" and stop:

```bash
$ cargo run --example sample_agent 1
```
