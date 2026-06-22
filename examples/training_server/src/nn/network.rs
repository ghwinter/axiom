//! 神经网络——多层感知机（MLP）。

use super::layer::LinearLayer;
use ndarray::Array1;

/// MLP: input → hidden1(ReLU) → hidden2(ReLU) → output(线性)
pub struct Network {
    pub layer1: LinearLayer,
    pub layer2: LinearLayer,
    pub layer3: LinearLayer,
}

impl Network {
    pub fn new(input_size: usize, hidden1: usize, hidden2: usize, output_size: usize) -> Self {
        Self {
            layer1: LinearLayer::new(input_size, hidden1, true),
            layer2: LinearLayer::new(hidden1, hidden2, true),
            layer3: LinearLayer::new(hidden2, output_size, false),
        }
    }

    /// 前向传播。
    pub fn forward(&mut self, x: &Array1<f64>) -> Array1<f64> {
        let h1 = self.layer1.forward(x);
        let h2 = self.layer2.forward(&h1);
        self.layer3.forward(&h2)
    }

    /// 反向传播（MSE 损失）。
    /// pred: 预测值, target: 真实值
    /// 返回：输入端的梯度（通常不需要使用）
    pub fn backward(&mut self, pred: &Array1<f64>, target: &Array1<f64>) {
        // MSE: loss = mean((pred - target)^2)
        // d_loss/d_pred = 2 * (pred - target) / n
        let n = pred.len() as f64;
        let grad_out = (pred - target) * (2.0 / n);
        let grad_h2 = self.layer3.backward(&grad_out);
        let grad_h1 = self.layer2.backward(&grad_h2);
        let _grad_in = self.layer1.backward(&grad_h1);
    }

    /// 清零所有层的梯度。
    pub fn zero_grad(&mut self) {
        self.layer1.zero_grad();
        self.layer2.zero_grad();
        self.layer3.zero_grad();
    }

    /// 将所有权重扁平化（用于持久化和 ModelDelta）。
    pub fn weights_flatten(&self) -> Vec<f64> {
        let mut v = Vec::new();
        v.extend(self.layer1.weights_flatten());
        v.extend(self.layer2.weights_flatten());
        v.extend(self.layer3.weights_flatten());
        v
    }

    /// 从扁平化 Vec 恢复权重。
    pub fn weights_from_flatten(&mut self, data: &[f64]) {
        let l1_len = self.layer1.weights_flatten().len();
        let l2_len = self.layer2.weights_flatten().len();
        self.layer1.weights_from_flatten(&data[..l1_len]);
        self.layer2.weights_from_flatten(&data[l1_len..l1_len + l2_len]);
        self.layer3.weights_from_flatten(&data[l1_len + l2_len..]);
    }

    /// 参数总数。
    pub fn param_count(&self) -> usize {
        self.layer1.weights_flatten().len()
            + self.layer2.weights_flatten().len()
            + self.layer3.weights_flatten().len()
    }
}
