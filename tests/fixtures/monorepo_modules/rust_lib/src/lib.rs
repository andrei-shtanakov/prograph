pub mod policy;
pub mod storage;

use crate::policy::Decider;
use crate::storage::Store;

pub struct PublicService {
    decider: Decider,
    store: Store,
}

pub fn build_service() -> PublicService {
    PublicService {
        decider: Decider::new(),
        store: Store::new(),
    }
}

fn internal_helper() -> i64 {
    0
}
