//! SGD 优化器（带动量）。

use super::layer::LinearLayer;
use ndarray::ScalarOperand;

pub struct SgdOptimizer {
    pub learning_rate: f64,
    pub momentum: f64,
    velocities_w: Vec<ndarray::Array2<f64>>,
    velocities_b: Vec<ndarray::Array1<f64>>,
}

impl SgdOptimizer {
    pub fn new(learning_rate: f64, momentum: f64) -> Self {
        Self {
            learning_rate,
            momentum,
            velocities_w: Vec::new(),
            velocities_b: Vec::new(),
        }
    }

    pub fn init_buffers(&mut self, layers: &[&LinearLayer]) {
        self.velocities_w.clear();
        self.velocities_b.clear();
        for layer in layers {
            self.velocities_w.push(ndarray::Array2::zeros(layer.weights.raw_dim()));
            self.velocities_b.push(ndarray::Array1::zeros(layer.bias.raw_dim()));
        }
    }

    /// v = momentum * v - lr * grad; w += v
    pub fn step_layer(&mut self, layer_idx: usize, layer: &mut LinearLayer) {
        let lr = self.learning_rate;
        let m = self.momentum;

        let v_w = &mut self.velocities_w[layer_idx];
        let v_b = &mut self.velocities_b[layer_idx];

        // v_w = m * v_w - lr * grad_w
        let new_vw = v_w.mapv(|x| x * m) - &layer.grad_w * lr;
        // v_b = m * v_b - lr * grad_b
        let new_vb = v_b.mapv(|x| x * m) - &layer.grad_b * lr;

        *v_w = new_vw;
        *v_b = new_vb;

        // 应用更新
        layer.apply_update(v_w, v_b);
    }
}
