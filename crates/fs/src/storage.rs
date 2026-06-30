use opendal::{Operator, services::Memory};

pub struct OpendalFileStorage {
    pub op: Operator,
}

impl OpendalFileStorage {
    // TODO: change to use fs
    pub fn new() -> Self {
        let op = Operator::new(Memory::default()).unwrap().finish();
        Self { op }
    }

    pub fn new_in_memory() -> Self {
        let op = Operator::new(Memory::default()).unwrap().finish();
        Self { op }
    }
}
