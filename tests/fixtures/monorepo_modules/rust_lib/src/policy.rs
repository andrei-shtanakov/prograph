pub struct Decider;

impl Decider {
    pub fn new() -> Self {
        Self
    }
}

pub trait Decidable {
    fn decide(&self) -> bool;
}
