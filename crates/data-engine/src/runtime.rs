use tokio::{sync::mpsc, task::JoinHandle};

use crate::data_engine::DataEngine;
use crate::runtime::error::Result;
use crate::runtime::types::DataEngineCmd;

pub mod error;
pub mod types;

pub struct DataEngineServer {
    engine: DataEngine,
    rx: mpsc::UnboundedReceiver<DataEngineCmd>,
}

impl DataEngineServer {
    /// Main event loop. Exits when all senders are dropped.
    pub async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            self.handle(cmd).await;
        }
    }

    async fn handle(&mut self, cmd: DataEngineCmd) {
        match cmd {
            DataEngineCmd::AddSourceNode { id, source, output_df_name, reply } => {
                let _ = reply.send(self.engine.source_node(id, source, output_df_name).map(|_| ()));
            }
            DataEngineCmd::AddSqlNode { id, query, output_df_name, reply } => {
                let _ = reply.send(self.engine.sql_node(id, query, output_df_name).map(|_| ()));
            }
            DataEngineCmd::AddSinkNode { id, sink, reply } => {
                let _ = reply.send(self.engine.sink_node(id, sink).map(|_| ()));
            }
            DataEngineCmd::AddEdge {
                from,
                to,
                reply,
            } => {
                let _ = reply.send(self.engine.add_edge(from, to).map(|_| ()));
            }
            DataEngineCmd::RunDag { reply } => {
                let _ = reply.send(self.engine.run().await);
            }
            DataEngineCmd::GetOutput { id, reply } => {
                let _ = reply.send(Ok(self.engine.get_output(id).await));
            }
            DataEngineCmd::RemoveNode { id, reply } => {
                let _ = reply.send(self.engine.remove_node(id).map(|_| ()));
            }
        }
    }
}

#[derive(Clone)]
pub struct DataEngineClient {
    tx: mpsc::UnboundedSender<DataEngineCmd>,
}

impl DataEngineClient {
    async fn request<T>(
        &self,
        cmd: DataEngineCmd,
        reply_rx: tokio::sync::oneshot::Receiver<
            std::result::Result<T, crate::data_engine::error::Error>,
        >,
    ) -> Result<T> {
        use crate::runtime::error::ClientError;

        self.tx.send(cmd).map_err(|_| ClientError::ServerClosed)?;
        reply_rx
            .await
            .map_err(|_| ClientError::ServerClosed)?
            .map_err(Into::into)
    }

    pub async fn add_source_node(
        &self,
        id: String,
        source: crate::data_engine::Source,
        output_df_name: String,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddSourceNode {
                id,
                source,
                output_df_name,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn add_sql_node(&self, id: String, query: String, output_df_name: String) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddSqlNode {
                id,
                query,
                output_df_name,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn add_sink_node(&self, id: String, sink: crate::data_engine::Sink) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddSinkNode {
                id,
                sink,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn add_edge(&self, from: String, to: String) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddEdge {
                from,
                to,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn run_dag(&self) -> Result<crate::data_engine::dag::RunReport> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(DataEngineCmd::RunDag { reply: reply_tx }, reply_rx)
            .await
    }

    pub async fn get_output(&self, id: String) -> Result<Option<crate::data_engine::dag::graph::NamedDataFrames>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::GetOutput {
                id,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn remove_node(&self, id: String) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::RemoveNode {
                id,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }
}

pub fn spawn_engine() {}

/// Spawn server through dependency injection, good for test purpose.
pub fn spawn_with_engine(engine: DataEngine) -> (DataEngineClient, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel::<DataEngineCmd>();
    let server = DataEngineServer { engine, rx };
    let client = DataEngineClient { tx };
    let handle = tokio::task::spawn(server.run());

    (client, handle)
}
