//! sqlmini 数据层：CSV 行加载 → `Vec<Record>`（值行）。
//!
//! 简化 CSV：逗号分隔、整行无引号包裹（含引号值不支持——诚实范围）、
//! 首行为列名（须与 schema 一致）。类型由 schema 转换（Int/Float/Str/Bool），
//! 转换失败 = 类型化 [`SqlError::Exec`]（数据面错误，非静默默认）。

use crate::ast::Value;
use crate::errors::SqlError;
use crate::schema::Schema;

/// 数据行：按 schema 列序的值。
pub type Record = Vec<Value>;

/// 加载 CSV 文本（外部文件或测试内联），按 schema 转值。
pub fn load_csv(schema: &Schema, csv: &str) -> Result<Vec<Record>, SqlError> {
    let mut records = Vec::new();
    let mut lines = csv.lines();
    let header = lines
        .next()
        .ok_or_else(|| SqlError::Exec("csv".to_string(), "空文件：缺表头".to_string()))?;
    let header_cols: Vec<&str> = header.split(',').map(str::trim).collect();
    for (i, h) in header_cols.iter().enumerate() {
        if schema.index(h).is_none() {
            return Err(SqlError::Exec(
                "csv".to_string(),
                format!("表头列 #{i} '{h}' 不在 schema 中"),
            ));
        }
    }
    for (lineno, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() != schema.cols.len() {
            return Err(SqlError::Exec(
                "csv".to_string(),
                format!("第 {} 行字段数 {} ≠ schema 列数 {}", lineno + 2, fields.len(), schema.cols.len()),
            ));
        }
        let mut rec = Vec::with_capacity(fields.len());
        for (i, field) in fields.iter().enumerate() {
            let name = &schema.cols[i].0;
            let ty = schema.cols[i].1;
            let v = parse_field(name, ty, field)?;
            rec.push(v);
        }
        records.push(rec);
    }
    Ok(records)
}

fn parse_field(col: &str, ty: crate::schema::ColType, raw: &str) -> Result<Value, SqlError> {
    match ty {
        crate::schema::ColType::Int => raw
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| SqlError::Exec("csv".to_string(), format!("列 '{col}' 非整数: {raw:?}"))),
        crate::schema::ColType::Float => raw
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| SqlError::Exec("csv".to_string(), format!("列 '{col}' 非浮点: {raw:?}"))),
        crate::schema::ColType::Str => Ok(Value::Str(raw.to_string())),
        crate::schema::ColType::Bool => match raw.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(SqlError::Exec(
                "csv".to_string(),
                format!("列 '{col}' 非布尔: {raw:?}"),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColType, Schema};

    fn demo_schema() -> Schema {
        Schema::from_columns(
            "e",
            vec![
                ("id".to_string(), ColType::Int),
                ("dept".to_string(), ColType::Str),
                ("salary".to_string(), ColType::Int),
            ],
        )
    }

    #[test]
    fn loads_typed_rows_from_csv() {
        let s = demo_schema();
        let rows = load_csv(&s, "id,dept,salary\n1,eng,100\n2,ops,200\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec![Value::Int(1), Value::Str("eng".to_string()), Value::Int(100)]);
        assert_eq!(rows[1][2], Value::Int(200));
    }

    #[test]
    fn header_mismatch_is_typed_error() {
        let s = demo_schema();
        let e = load_csv(&s, "id,dept,money\n1,eng,100\n").expect_err("header mismatch");
        assert_eq!(e.stage(), "exec");
        assert!(matches!(e, SqlError::Exec(..)));
    }

    #[test]
    fn field_type_mismatch_is_typed_error_not_silent_zero() {
        let s = demo_schema();
        let e = load_csv(&s, "id,dept,salary\n1,eng,abc\n").expect_err("bad int");
        assert!(matches!(e, SqlError::Exec(..)), "abc → Int 列报错，不静默成 0");
    }
}