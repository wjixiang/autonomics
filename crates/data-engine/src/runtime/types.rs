use datafusion::prelude::DataFrame;
use tokio::sync::oneshot;

use crate::data_engine::dag::RunReport;
use crate::data_engine::error::Result as EngineResult;
use crate::data_engine::{Sink, Source};

pub enum DataEngineCmd {
    AddSourceNode {
        id: String,
        source: Source,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddSqlNode {
        id: String,
        query: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddSinkNode {
        id: String,
        sink: Sink,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddEdge {
        from: String,
        to: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    AddNamedEdge {
        from: String,
        to: String,
        port: String,
        reply: oneshot::Sender<EngineResult<()>>,
    },
    RunDag {
        reply: oneshot::Sender<EngineResult<RunReport>>,
    },
    GetOutput {
        id: String,
        reply: oneshot::Sender<EngineResult<Option<Vec<DataFrame>>>>,
    },
}
