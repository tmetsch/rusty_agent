use crate::agent;
use std::sync;
use std::vec;

///
/// An struct to help create a new Agent.
///
pub struct AgentBuilder {
    ep: String,
    peers: sync::Arc<sync::Mutex<Vec<String>>>,
    msgs: sync::Arc<sync::Mutex<Vec<String>>>,
    ctxt: zmq::Context,
    wait: u64,
    timeout: u64,
}

/// Builder for creating new agents.
impl AgentBuilder {
    /// Creates a new agent.
    pub fn new(ep: String) -> Self {
        let ngbhs: sync::Arc<sync::Mutex<Vec<String>>> =
            sync::Arc::new(sync::Mutex::new(vec![ep.clone()]));
        let msgs: sync::Arc<sync::Mutex<Vec<String>>> = sync::Arc::new(sync::Mutex::new(vec![]));
        let context: zmq::Context = zmq::Context::new();
        Self {
            ep,
            peers: ngbhs,
            ctxt: context,
            msgs,
            wait: 100,
            timeout: 2,
        }
    }

    /// Set the timeout between pings to neighbours in seconds.
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the wait time in ms for a neighbour to respond.
    pub fn wait(mut self, wait: u64) -> Self {
        self.wait = wait;
        self
    }

    /// Create the actual ZeroAgent.
    pub fn build(self) -> agent::ZeroAgent {
        let Self {
            ep,
            peers,
            ctxt,
            msgs,
            wait,
            timeout,
        } = self;
        agent::ZeroAgent {
            ep,
            peers,
            ctxt,
            msgs,
            wait,
            timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ZeroAgent;

    // Test for success.

    #[test]
    fn test_build_for_success() {
        AgentBuilder::new("inproc://#0".to_string()).build();
    }

    #[test]
    fn test_timeout_for_success() {
        AgentBuilder::new("inproc://#0".to_string())
            .timeout(10)
            .build();
    }

    #[test]
    fn test_wait_for_success() {
        AgentBuilder::new("inproc://#0".to_string())
            .wait(10)
            .build();
    }

    // Test for failure.

    // Test for sanity.

    #[test]
    fn test_build_for_sanity() {
        let agent: ZeroAgent = AgentBuilder::new("inproc://#0".to_string())
            .timeout(1)
            .wait(2)
            .build();
        assert_eq!(agent.timeout, 1);
        assert_eq!(agent.wait, 2);
    }
}
