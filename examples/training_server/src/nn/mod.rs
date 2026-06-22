//! 神经网络模块——简单的多层感知机（MLP）+ SGD 优化器。

pub mod layer;
pub mod network;
pub mod sgd;

pub use layer::LinearLayer;
pub use network::Network;
pub use sgd::SgdOptimizer;
