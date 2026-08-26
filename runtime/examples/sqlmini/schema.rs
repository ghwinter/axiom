//! sqlmini 表结构（schema）：单表子集的列类型与注册。
//!
//! `PortCell::State: Default` 的约束下，Planner 以 [`Schema`] 作状态：
//! 空表（Default）视为未注册——驱动方先以 [`Schema::from_columns`] 载入，
//! step 对未注册表给出 [`SqlError::Plan`]（诚实：无 schema 不猜测）。

/// 列类型（执行期值类型检查用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColType {
    Int,
    Float,
    Str,
    Bool,
}

impl ColType {
    /// 是否为数值（数字运算可参与）。
    pub fn is_numeric(&self) -> bool {
        matches!(self, ColType::Int | ColType::Float)
    }
}

/// 表结构：表名 + 有序列。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    pub table: String,
    pub cols: Vec<(String, ColType)>,
}

impl Schema {
    /// 空表（未注册）。
    pub fn empty() -> Self {
        Schema::default()
    }

    /// 注册一张表。
    pub fn from_columns(table: &str, cols: Vec<(String, ColType)>) -> Self {
        Schema {
            table: table.to_string(),
            cols,
        }
    }

    /// 是否已注册（非空表）。
    pub fn is_registered(&self) -> bool {
        !self.table.is_empty()
    }

    /// 列名 → 位置。
    pub fn index(&self, name: &str) -> Option<usize> {
        self.cols.iter().position(|(n, _)| n == name)
    }

    /// 列类型。
    pub fn col_type(&self, name: &str) -> Option<ColType> {
        self.index(name).map(|i| self.cols[i].1)
    }

    /// 列名序列（执行期行布局）。
    pub fn col_names(&self) -> Vec<String> {
        self.cols.iter().map(|(n, _)| n.clone()).collect()
    }
}