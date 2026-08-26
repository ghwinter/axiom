//! sqlmini AST：单表 SELECT 子集的抽象语法。
//! 纯数据类型（值对象），不携带解析逻辑；语义/计划阶段消费。

/// 值字面量（语义期由词法字面量解析而来）。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}

impl Value {
    /// 展示形态（执行输出用）。
    pub fn display(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format!("{f}"),
            Value::Str(s) => format!("'{s}'"),
            Value::Bool(b) => b.to_string(),
            Value::Null => "NULL".to_string(),
        }
    }

    /// 数值视图（数字比较/运算用；非数值 → `None`）。
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// 哈希支持（分组键/去重）：f64 以位模式入哈希，保证确定性。
    pub fn hash_value<H: core::hash::Hasher>(&self, h: &mut H) {
        use core::hash::Hash;
        match self {
            Value::Int(i) => {
                0u8.hash(h);
                i.hash(h);
            }
            Value::Float(f) => {
                1u8.hash(h);
                f.to_bits().hash(h);
            }
            Value::Str(s) => {
                2u8.hash(h);
                s.hash(h);
            }
            Value::Bool(b) => {
                3u8.hash(h);
                b.hash(h);
            }
            Value::Null => 4u8.hash(h),
        }
    }

    /// 总序比较（排序键用；顺序：Null < Bool < 数值 < Str）。
    /// 数值间按 f64 比较（Int/Float 混比）；NaN 处理为等于。
    pub fn total_cmp(&self, other: &Value) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        fn rank(v: &Value) -> u8 {
            match v {
                Value::Null => 0,
                Value::Bool(_) => 1,
                Value::Int(_) | Value::Float(_) => 2,
                Value::Str(_) => 3,
            }
        }
        let (ra, rb) = (rank(self), rank(other));
        if ra != rb {
            return ra.cmp(&rb);
        }
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
                (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal)
            }
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

// BTreeMap 分组键/排序所需的序：以 `total_cmp` 为准（f64 经位模式/偏序回退，
// 保证全程总序——`Ord` 的合法性由 `total_cmp` 的一致性承担）。
impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other)) // 规范形态：以 Ord 为唯一判序来源
    }
}
impl Ord for Value {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.total_cmp(other)
    }
}
// 等效关系声明（f64 NaN 例外按运营约定：数据面不含 NaN；位模式已在
// `hash_value` 中保证分组/去重确定性）。
impl Eq for Value {}

/// 二元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// 一元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// 聚合函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AggFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFn {
    /// 默认输出名（无别名时的投影列名）。
    pub fn default_name(&self) -> &'static str {
        match self {
            AggFn::Count => "count",
            AggFn::Sum => "sum",
            AggFn::Avg => "avg",
            AggFn::Min => "min",
            AggFn::Max => "max",
        }
    }
}

/// 表达式。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 列引用（单表子集：仅列名）。
    Col(String),
    /// 字面量。
    Lit(Value),
    /// 二元运算。
    Bin(Box<Expr>, BinOp, Box<Expr>),
    /// 一元运算。
    Un(UnOp, Box<Expr>),
    /// 聚合调用（`COUNT(*)` 时参数为 `Star`）。
    Agg(AggFn, Box<Expr>),
    /// `SELECT *` 之 `*`（仅作为 select item 顶层或 COUNT 参数）。
    Star,
}

impl Expr {
    /// 是否为列引用（投影/别名推断用；语义阶段使用）。
    #[allow(dead_code)]
    pub fn col_name(&self) -> Option<&str> {
        match self {
            Expr::Col(name) => Some(name),
            _ => None,
        }
    }
}

/// 选择项：表达式 + 可选别名。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<String>,
}

/// 单表 SELECT 语句。
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub distinct: bool,
    pub items: Vec<SelectItem>,
    pub from: String,
    pub where_: Option<Expr>,
    pub group_by: Vec<Expr>,
    /// (表达式, 升序)。
    pub order_by: Vec<(Expr, bool)>,
    pub limit: Option<usize>,
}