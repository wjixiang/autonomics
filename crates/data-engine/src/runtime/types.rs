use tokio::sync::oneshot;

use crate::data_engine::dag::RunReport;
use crate::data_engine::dag::graph::PortOutputs;
use crate::data_engine::error::Result as EngineResult;
use crate::data_engine::{Sink, Source};

pub enum DataEngineCmd {
    AddSourceNode {
        id: String,
        source: Source,
        output_df_name: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddSqlNode {
        id: String,
        query: String,
        output_df_name: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddSinkNode {
        id: String,
        sink: Sink,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddLinearRegressionNode {
        id: String,
        x_columns: Vec<String>,
        y_column: String,
        intercept: bool,
        output_df_name: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddEdge {
        from: String,
        from_port: Option<u8>,
        to: String,
        to_port: Option<u8>,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    RunDag {
        reply: oneshot::Sender<EngineResult<RunReport>>,
    },
    GetOutput {
        id: String,
        reply: oneshot::Sender<EngineResult<Option<PortOutputs>>>,
    },
    RemoveNode {
        id: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    ViewDag {
        reply: oneshot::Sender<EngineResult<String>>,
    },
    ClearDag {
        reply: oneshot::Sender<EngineResult<()>>,
    },
}
