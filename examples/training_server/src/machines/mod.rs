//! Machine 实现——每个模块对应一个 axiom Machine。

pub mod loader;
pub mod batcher;
pub mod trainer;
pub mod evaluator;
pub mod checkpointer;
pub mod observer;
pub mod controller;

// 重新导出主要类型
pub use loader::{DataLoader, DataLoaderPorts, DataLoaderInput, DataLoaderOutput};
pub use batcher::{Batcher, BatcherPorts, BatcherInput, BatcherOutput};
pub use trainer::{Trainer, TrainerPorts, TrainerInput, TrainerOutput};
pub use evaluator::{Evaluator, EvaluatorPorts, EvaluatorInput, EvaluatorOutput};
pub use checkpointer::{Checkpointer, CheckpointerPorts, CheckpointerInput, CheckpointerOutput};
pub use observer::{Observer, ObserverPorts, ObserverInput, ObserverOutput};
pub use controller::{Controller, ControllerPorts, ControllerInput, ControllerOutput};
