use tokio::{sync::mpsc, task::JoinHandle};

use crate::data_engine::IcebergDataEngine;
use crate::runtime::error::Result;
use crate::runtime::types::DataEngineCmd;

pub mod error;
pub mod types;

pub struct DataEngineServer {
    engine: IcebergDataEngine,
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
            DataEngineCmd::AddSourceNode {
                id,
                source,
                reply,
            } => {
                let _ = reply.send(
                    self.engine
                        .source_node(id, source)
                        .map(|_| ()),
                );
            }
            DataEngineCmd::AddSqlNode { id, query, reply } => {
                let _ = reply.send(self.engine.sql_node(id, query).map(|_| ()));
            }
            DataEngineCmd::AddSinkNode { id, sink, reply } => {
                let _ = reply.send(self.engine.sink_node(id, sink).map(|_| ()));
            }
            DataEngineCmd::AddLinearRegressionNode {
                id,
                x_columns,
                y_column,
                intercept,
                reply,
            } => {
                let _ = reply.send(
                    self.engine
                        .linear_regression_node(id, x_columns, y_column, intercept)
                        .map(|_| ()),
                );
            }
            DataEngineCmd::AddLdscNode {
                id,
                datalake,
                z_column,
                n_column,
                rsid_column,
                ld_score_table,
                m,
                n_blocks,
                intercept,
                reply,
            } => {
                let _ = reply.send(
                    self.engine
                        .ldsc_node(
                            id,
                            datalake,
                            z_column,
                            n_column,
                            rsid_column,
                            ld_score_table,
                            m,
                            n_blocks,
                            intercept,
                        )
                        .map(|_| ()),
                );
            }
            DataEngineCmd::AddEdge {
                from,
                from_port,
                to,
                to_port,
                reply,
            } => {
                let res = match (from_port, to_port) {
                    (Some(fp), Some(tp)) => self.engine.add_edge(from, to, fp, tp).map(|_| ()),
                    (None, None) => self.engine.add_edge(from, to, 0, 0).map(|_| ()),
                    _ => Err(crate::data_engine::error::Error::Custom(
                        "add_edge: from_port and to_port must both be Some or both None"
                            .to_string(),
                    )),
                };
                let _ = reply.send(res);
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
            DataEngineCmd::ViewDag { reply } => {
                let _ = reply.send(self.engine.view_dag());
            }
            DataEngineCmd::ClearDag { reply } => {
                let _ = reply.send(self.engine.clear_dag().map(|_| ()));
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
    ) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddSourceNode {
                id,
                source,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn add_sql_node(&self, id: String, query: String) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddSqlNode {
                id,
                query,
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

    pub async fn add_linear_regression_node(
        &self,
        id: String,
        x_columns: Vec<String>,
        y_column: String,
        intercept: bool,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddLinearRegressionNode {
                id,
                x_columns,
                y_column,
                intercept,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    pub async fn add_ldsc_node(
        &self,
        id: String,
        datalake: std::sync::Arc<datalake::Datalake>,
        z_column: String,
        n_column: String,
        rsid_column: String,
        ld_score_table: String,
        m: Vec<f64>,
        n_blocks: usize,
        intercept: Option<f64>,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddLdscNode {
                id,
                datalake,
                z_column,
                n_column,
                rsid_column,
                ld_score_table,
                m,
                n_blocks,
                intercept,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    /// Connect two nodes using their default (single) ports.
    pub async fn add_edge(&self, from: String, to: String) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddEdge {
                from,
                from_port: None,
                to,
                to_port: None,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    /// Connect `from`'s `from_port` output to `to`'s `to_port` input.
    pub async fn add_edge_port(
        &self,
        from: String,
        from_port: u8,
        to: String,
        to_port: u8,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(
            DataEngineCmd::AddEdge {
                from,
                from_port: Some(from_port),
                to,
                to_port: Some(to_port),
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

    pub async fn get_output(
        &self,
        id: String,
    ) -> Result<Option<crate::data_engine::dag::graph::PortOutputs>> {
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

    pub async fn view_dag(&self) -> Result<String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(DataEngineCmd::ViewDag { reply: reply_tx }, reply_rx)
            .await
    }

    pub async fn clear_dag(&self) -> Result<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.request(DataEngineCmd::ClearDag { reply: reply_tx }, reply_rx)
            .await
    }
}

pub fn spawn_engine() {}

/// Spawn server through dependency injection, good for test purpose.
pub fn spawn_with_engine(engine: IcebergDataEngine) -> (DataEngineClient, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel::<DataEngineCmd>();
    let server = DataEngineServer { engine, rx };
    let client = DataEngineClient { tx };
    let handle = tokio::task::spawn(server.run());

    (client, handle)
}
