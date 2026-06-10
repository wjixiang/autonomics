use agentik_types::messages::ContentBlock;

/// Commands sent to a managed agent's control loop.
#[derive(Debug)]
pub enum Command {
    /// Start the agent (calls `agent.start()`).
    Start,

    /// Cancel the currently running agent task.
    Stop,

    /// Restart: cancel, rebuild from the stored builder, then start again.
    Restart,

    /// Inject a user message into the agent's memory.
    InjectMessage(Vec<ContentBlock>),
}
