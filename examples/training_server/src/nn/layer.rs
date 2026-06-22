//! 线性层：y = W·x + b，带 ReLU 激活（可选）。

use ndarray::{Array1, Array2};

/// 线性全连接层。
pub struct LinearLayer {
    pub weights: Array2<f64>, // [out_size, in_size]
    pub bias: Array1<f64>,    // [out_size]
    pub grad_w: Array2<f64>,  // 权重梯度
    pub grad_b: Array1<f64>,  // 偏置梯度
    pub input_cache: Array1<f64>,
    pub use_relu: bool,
}

impl LinearLayer {
    pub fn new(in_size: usize, out_size: usize, use_relu: bool) -> Self {
        let scale = (2.0 / (in_size + out_size) as f64).sqrt();
        let mut weights = Array2::zeros((out_size, in_size));
        for i in 0..out_size {
            for j in 0..in_size {
                weights[[i, j]] = rand_normal() * scale;
            }
        }
        Self {
            weights,
            bias: Array1::zeros(out_size),
            grad_w: Array2::zeros((out_size, in_size)),
            grad_b: Array1::zeros(out_size),
            input_cache: Array1::zeros(in_size),
            use_relu,
        }
    }

    pub fn forward(&mut self, x: &Array1<f64>) -> Array1<f64> {
        self.input_cache = x.clone();
        let mut y = self.weights.dot(x) + &self.bias;
        if self.use_relu {
            for v in y.iter_mut() {
                if *v < 0.0 { *v = 0.0; }
            }
        }
        y
    }

    pub fn backward(&mut self, grad_y: &Array1<f64>) -> Array1<f64> {
        let mut grad_y = grad_y.clone();
        if self.use_relu {
            let y = self.weights.dot(&self.input_cache) + &self.bias;
            for i in 0..grad_y.len() {
                if y[i] <= 0.0 { grad_y[i] = 0.0; }
            }
        }
        self.grad_b += &grad_y;
        // grad_x = W^T · grad_y（先算，避免 grad_y 被 move）
        let grad_x = self.weights.t().dot(&grad_y);
        // grad_w += grad_y (outer) input_cache
        let input = self.input_cache.clone();
        let grad_y_col = grad_y.insert_axis(ndarray::Axis(1));
        let input_row = input.insert_axis(ndarray::Axis(0));
        self.grad_w += &grad_y_col.dot(&input_row);
        grad_x
    }

    pub fn apply_update(&mut self, delta_w: &Array2<f64>, delta_b: &Array1<f64>) {
        self.weights += delta_w;
        self.bias += delta_b;
    }

    pub fn weights_flatten(&self) -> Vec<f64> {
        self.weights.iter().chain(self.bias.iter()).cloned().collect()
    }

    pub fn weights_from_flatten(&mut self, data: &[f64]) {
        let w_len = self.weights.len();
        let ncols = self.weights.ncols();
        for (i, v) in data[..w_len].iter().enumerate() {
            self.weights[(i / ncols, i % ncols)] = *v;
        }
        for (i, v) in data[w_len..].iter().enumerate() {
            self.bias[i] = *v;
        }
    }

    pub fn zero_grad(&mut self) {
        self.grad_w.fill(0.0);
        self.grad_b.fill(0.0);
    }
}

fn rand_normal() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(42);
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        ((x as f64 / u64::MAX as f64) - 0.5) * 3.0
    })
}
