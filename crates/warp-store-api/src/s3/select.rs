//! S3 Select implementation
//!
//! Enables SQL queries on objects stored in S3-compatible storage.
//! Supports CSV and JSON input formats with SQL WHERE/SELECT filtering.
//! Parquet support is planned via RustySpark integration.

#[cfg(feature = "s3-select")]
use std::io::Cursor;

#[cfg(feature = "s3-select")]
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
#[cfg(feature = "s3-select")]
use bytes::Bytes;
#[cfg(feature = "s3-select")]
use serde::Deserialize;

#[cfg(feature = "s3-select")]
use warp_store::backend::StorageBackend;
#[cfg(feature = "s3-select")]
use warp_store::ObjectKey;

#[cfg(feature = "s3-select")]
use crate::error::{ApiError, ApiResult};
#[cfg(feature = "s3-select")]
use crate::AppState;

// =============================================================================
// Request/Response Types
// =============================================================================

/// Query parameters for S3 Select
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize)]
pub struct SelectQuery {
    /// Presence indicates this is a SELECT request
    #[serde(rename = "select")]
    pub select: Option<String>,
    /// Select type (currently only "2" is supported)
    #[serde(rename = "select-type")]
    pub select_type: Option<String>,
}

/// S3 Select request body
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SelectObjectContentRequest {
    /// SQL expression (e.g., "SELECT * FROM s3object WHERE age > 25")
    pub expression: String,
    /// Expression type (must be "SQL")
    pub expression_type: String,
    /// Input format configuration
    pub input_serialization: InputSerialization,
    /// Output format configuration
    pub output_serialization: OutputSerialization,
    /// Optional request progress
    #[serde(default)]
    pub request_progress: Option<RequestProgress>,
    /// Optional scan range
    #[serde(default)]
    pub scan_range: Option<ScanRange>,
}

/// Input serialization format
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InputSerialization {
    /// Compression type (NONE, GZIP, BZIP2)
    #[serde(default)]
    pub compression_type: Option<String>,
    /// CSV format options
    #[serde(rename = "CSV")]
    pub csv: Option<CsvInput>,
    /// JSON format options
    #[serde(rename = "JSON")]
    pub json: Option<JsonInput>,
    /// Parquet format options (no configuration needed)
    #[serde(rename = "Parquet")]
    pub parquet: Option<ParquetInput>,
}

/// CSV input options
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct CsvInput {
    /// File header info: USE, IGNORE, or NONE
    #[serde(default = "default_file_header_info")]
    pub file_header_info: String,
    /// Field delimiter (default: ,)
    #[serde(default = "default_field_delimiter")]
    pub field_delimiter: String,
    /// Record delimiter (default: \n)
    #[serde(default = "default_record_delimiter")]
    pub record_delimiter: String,
    /// Quote character (default: ")
    #[serde(default = "default_quote_character")]
    pub quote_character: String,
    /// Quote escape character
    #[serde(default)]
    pub quote_escape_character: Option<String>,
    /// Comments character
    #[serde(default)]
    pub comments: Option<String>,
    /// Allow quoted record delimiter
    #[serde(default)]
    pub allow_quoted_record_delimiter: Option<bool>,
}

#[cfg(feature = "s3-select")]
fn default_file_header_info() -> String {
    "NONE".to_string()
}

#[cfg(feature = "s3-select")]
fn default_field_delimiter() -> String {
    ",".to_string()
}

#[cfg(feature = "s3-select")]
fn default_record_delimiter() -> String {
    "\n".to_string()
}

#[cfg(feature = "s3-select")]
fn default_quote_character() -> String {
    "\"".to_string()
}

/// JSON input options
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct JsonInput {
    /// JSON type: DOCUMENT or LINES
    #[serde(rename = "Type", default = "default_json_type")]
    pub json_type: String,
}

#[cfg(feature = "s3-select")]
fn default_json_type() -> String {
    "LINES".to_string()
}

/// Parquet input options (minimal - format is self-describing)
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ParquetInput {}

/// Output serialization format
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct OutputSerialization {
    /// CSV output options
    #[serde(rename = "CSV")]
    pub csv: Option<CsvOutput>,
    /// JSON output options
    #[serde(rename = "JSON")]
    pub json: Option<JsonOutput>,
}

/// CSV output options
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct CsvOutput {
    /// Field delimiter (default: ,)
    #[serde(default = "default_field_delimiter")]
    pub field_delimiter: String,
    /// Record delimiter (default: \n)
    #[serde(default = "default_record_delimiter")]
    pub record_delimiter: String,
    /// Quote character (default: ")
    #[serde(default = "default_quote_character")]
    pub quote_character: String,
    /// Quote escape character
    #[serde(default)]
    pub quote_escape_character: Option<String>,
    /// Quote fields: ALWAYS, ASNEEDED
    #[serde(default)]
    pub quote_fields: Option<String>,
}

/// JSON output options
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct JsonOutput {
    /// Record delimiter (default: \n)
    #[serde(default = "default_record_delimiter")]
    pub record_delimiter: String,
}

/// Request progress settings
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct RequestProgress {
    /// Whether to send progress events
    #[serde(default)]
    pub enabled: bool,
}

/// Scan range for partial object reads
#[cfg(feature = "s3-select")]
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ScanRange {
    /// Start byte offset
    pub start: Option<u64>,
    /// End byte offset
    pub end: Option<u64>,
}

// =============================================================================
// SQL Evaluator
// =============================================================================

#[cfg(feature = "s3-select")]
mod evaluator {
    use sqlparser::ast::{
        BinaryOperator, Expr, SelectItem, SetExpr, Statement, Value,
    };
    use sqlparser::dialect::GenericDialect;
    use sqlparser::parser::Parser;
    use std::collections::HashMap;

    /// Parsed SQL query for S3 Select
    #[derive(Debug)]
    pub struct ParsedQuery {
        /// Selected columns (None = SELECT *)
        pub projections: Option<Vec<String>>,
        /// WHERE clause predicate
        pub predicate: Option<Expr>,
        /// LIMIT clause
        pub limit: Option<usize>,
    }

    /// Parse an S3 Select SQL expression
    pub fn parse_sql(sql: &str) -> Result<ParsedQuery, String> {
        let dialect = GenericDialect {};
        let statements = Parser::parse_sql(&dialect, sql)
            .map_err(|e| format!("SQL parse error: {}", e))?;

        if statements.is_empty() {
            return Err("Empty SQL expression".to_string());
        }

        let statement = &statements[0];
        match statement {
            Statement::Query(query) => {
                let body = query.body.as_ref();
                match body {
                    SetExpr::Select(select) => {
                        // Extract projections
                        let projections = extract_projections(&select.projection)?;

                        // Extract WHERE clause
                        let predicate = select.selection.clone();

                        // Extract LIMIT
                        let limit = query.limit.as_ref().and_then(|l| {
                            if let Expr::Value(Value::Number(n, _)) = l {
                                n.parse().ok()
                            } else {
                                None
                            }
                        });

                        Ok(ParsedQuery {
                            projections,
                            predicate,
                            limit,
                        })
                    }
                    _ => Err("Only SELECT queries are supported".to_string()),
                }
            }
            _ => Err("Only SELECT queries are supported".to_string()),
        }
    }

    fn extract_projections(items: &[SelectItem]) -> Result<Option<Vec<String>>, String> {
        let mut columns = Vec::new();

        for item in items {
            match item {
                SelectItem::Wildcard(_) => return Ok(None), // SELECT *
                SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                    columns.push(ident.value.clone());
                }
                SelectItem::ExprWithAlias { expr: Expr::Identifier(ident), alias } => {
                    // Use alias if provided, otherwise column name
                    columns.push(alias.value.clone());
                    let _ = ident; // Original column name used for lookup
                }
                _ => {
                    // For complex expressions, we'll add them as-is
                    columns.push(format!("{}", item));
                }
            }
        }

        Ok(Some(columns))
    }

    /// Evaluate a row against a WHERE predicate
    pub fn evaluate_predicate(
        row: &HashMap<String, serde_json::Value>,
        predicate: &Expr,
    ) -> bool {
        match evaluate_expr(row, predicate) {
            Some(serde_json::Value::Bool(b)) => b,
            _ => false,
        }
    }

    fn evaluate_expr(
        row: &HashMap<String, serde_json::Value>,
        expr: &Expr,
    ) -> Option<serde_json::Value> {
        match expr {
            Expr::Identifier(ident) => {
                // Column reference - look up in row
                row.get(&ident.value).cloned()
            }
            Expr::Value(val) => {
                // Literal value
                match val {
                    Value::Number(n, _) => {
                        if let Ok(i) = n.parse::<i64>() {
                            Some(serde_json::Value::Number(i.into()))
                        } else if let Ok(f) = n.parse::<f64>() {
                            serde_json::Number::from_f64(f)
                                .map(serde_json::Value::Number)
                        } else {
                            None
                        }
                    }
                    Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => {
                        Some(serde_json::Value::String(s.clone()))
                    }
                    Value::Boolean(b) => Some(serde_json::Value::Bool(*b)),
                    Value::Null => Some(serde_json::Value::Null),
                    _ => None,
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let left_val = evaluate_expr(row, left)?;
                let right_val = evaluate_expr(row, right)?;
                evaluate_binary_op(&left_val, op, &right_val)
            }
            Expr::Nested(inner) => evaluate_expr(row, inner),
            Expr::IsNull(inner) => {
                let val = evaluate_expr(row, inner);
                Some(serde_json::Value::Bool(val.map_or(true, |v| v.is_null())))
            }
            Expr::IsNotNull(inner) => {
                let val = evaluate_expr(row, inner);
                Some(serde_json::Value::Bool(val.map_or(false, |v| !v.is_null())))
            }
            Expr::InList { expr, list, negated } => {
                let val = evaluate_expr(row, expr)?;
                let in_list = list.iter().any(|item| {
                    evaluate_expr(row, item).map_or(false, |v| values_equal(&val, &v))
                });
                Some(serde_json::Value::Bool(if *negated { !in_list } else { in_list }))
            }
            Expr::Between { expr, negated, low, high } => {
                let val = evaluate_expr(row, expr)?;
                let low_val = evaluate_expr(row, low)?;
                let high_val = evaluate_expr(row, high)?;
                let in_range = compare_values(&val, &low_val).map_or(false, |c| c >= 0)
                    && compare_values(&val, &high_val).map_or(false, |c| c <= 0);
                Some(serde_json::Value::Bool(if *negated { !in_range } else { in_range }))
            }
            Expr::Like { expr, pattern, negated, .. } => {
                let val = evaluate_expr(row, expr)?;
                let pattern_val = evaluate_expr(row, pattern)?;

                if let (serde_json::Value::String(s), serde_json::Value::String(p)) =
                    (&val, &pattern_val)
                {
                    let matches = like_match(s, p);
                    Some(serde_json::Value::Bool(if *negated { !matches } else { matches }))
                } else {
                    Some(serde_json::Value::Bool(false))
                }
            }
            _ => None, // Unsupported expression
        }
    }

    fn evaluate_binary_op(
        left: &serde_json::Value,
        op: &BinaryOperator,
        right: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        match op {
            // Comparison operators
            BinaryOperator::Eq => Some(serde_json::Value::Bool(values_equal(left, right))),
            BinaryOperator::NotEq => Some(serde_json::Value::Bool(!values_equal(left, right))),
            BinaryOperator::Lt => {
                Some(serde_json::Value::Bool(compare_values(left, right)? < 0))
            }
            BinaryOperator::LtEq => {
                Some(serde_json::Value::Bool(compare_values(left, right)? <= 0))
            }
            BinaryOperator::Gt => {
                Some(serde_json::Value::Bool(compare_values(left, right)? > 0))
            }
            BinaryOperator::GtEq => {
                Some(serde_json::Value::Bool(compare_values(left, right)? >= 0))
            }
            // Logical operators
            BinaryOperator::And => {
                let l = left.as_bool()?;
                let r = right.as_bool()?;
                Some(serde_json::Value::Bool(l && r))
            }
            BinaryOperator::Or => {
                let l = left.as_bool()?;
                let r = right.as_bool()?;
                Some(serde_json::Value::Bool(l || r))
            }
            // Arithmetic operators
            BinaryOperator::Plus => arithmetic_op(left, right, |a, b| a + b),
            BinaryOperator::Minus => arithmetic_op(left, right, |a, b| a - b),
            BinaryOperator::Multiply => arithmetic_op(left, right, |a, b| a * b),
            BinaryOperator::Divide => arithmetic_op(left, right, |a, b| if b != 0.0 { a / b } else { f64::NAN }),
            BinaryOperator::Modulo => arithmetic_op(left, right, |a, b| if b != 0.0 { a % b } else { f64::NAN }),
            // String operators
            BinaryOperator::StringConcat => {
                let l = left.as_str()?;
                let r = right.as_str()?;
                Some(serde_json::Value::String(format!("{}{}", l, r)))
            }
            _ => None,
        }
    }

    fn values_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
        match (left, right) {
            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                l.as_f64() == r.as_f64()
            }
            (serde_json::Value::String(l), serde_json::Value::String(r)) => l == r,
            (serde_json::Value::Bool(l), serde_json::Value::Bool(r)) => l == r,
            (serde_json::Value::Null, serde_json::Value::Null) => true,
            _ => false,
        }
    }

    fn compare_values(left: &serde_json::Value, right: &serde_json::Value) -> Option<i8> {
        match (left, right) {
            (serde_json::Value::Number(l), serde_json::Value::Number(r)) => {
                let lf = l.as_f64()?;
                let rf = r.as_f64()?;
                Some(if lf < rf { -1 } else if lf > rf { 1 } else { 0 })
            }
            (serde_json::Value::String(l), serde_json::Value::String(r)) => {
                Some(l.cmp(r) as i8)
            }
            _ => None,
        }
    }

    fn arithmetic_op<F>(
        left: &serde_json::Value,
        right: &serde_json::Value,
        op: F,
    ) -> Option<serde_json::Value>
    where
        F: Fn(f64, f64) -> f64,
    {
        let l = left.as_f64()?;
        let r = right.as_f64()?;
        let result = op(l, r);
        serde_json::Number::from_f64(result).map(serde_json::Value::Number)
    }

    fn like_match(s: &str, pattern: &str) -> bool {
        // Simple LIKE pattern matching (% = any chars, _ = single char)
        let regex_pattern = pattern
            .replace('%', ".*")
            .replace('_', ".");
        regex::Regex::new(&format!("^{}$", regex_pattern))
            .map(|re| re.is_match(s))
            .unwrap_or(false)
    }

    /// Project selected columns from a row
    pub fn project_row(
        row: &HashMap<String, serde_json::Value>,
        projections: &Option<Vec<String>>,
    ) -> HashMap<String, serde_json::Value> {
        match projections {
            None => row.clone(), // SELECT *
            Some(cols) => {
                let mut result = HashMap::new();
                for col in cols {
                    if let Some(val) = row.get(col) {
                        result.insert(col.clone(), val.clone());
                    }
                }
                result
            }
        }
    }
}

// =============================================================================
// Input Format Readers
// =============================================================================

#[cfg(feature = "s3-select")]
mod readers {
    use super::*;
    use std::collections::HashMap;

    /// Read CSV data into rows
    pub fn read_csv(
        data: &[u8],
        config: &CsvInput,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>, String> {
        let delimiter = config.field_delimiter.as_bytes().first().copied().unwrap_or(b',');

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(config.file_header_info.to_uppercase() == "USE")
            .flexible(true)
            .from_reader(Cursor::new(data));

        let headers: Vec<String> = if config.file_header_info.to_uppercase() == "USE" {
            reader
                .headers()
                .map_err(|e| format!("CSV header error: {}", e))?
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            // Generate column names: _1, _2, _3, ...
            let first_record = reader.records().next();
            if let Some(Ok(record)) = first_record {
                (1..=record.len()).map(|i| format!("_{}", i)).collect()
            } else {
                return Ok(vec![]);
            }
        };

        let mut rows = Vec::new();

        // Reset reader if we peeked at first record
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(config.file_header_info.to_uppercase() == "USE")
            .flexible(true)
            .from_reader(Cursor::new(data));

        for result in reader.records() {
            let record = result.map_err(|e| format!("CSV record error: {}", e))?;
            let mut row = HashMap::new();

            for (i, field) in record.iter().enumerate() {
                if i < headers.len() {
                    // Try to parse as number, otherwise keep as string
                    let value = if let Ok(n) = field.parse::<i64>() {
                        serde_json::Value::Number(n.into())
                    } else if let Ok(n) = field.parse::<f64>() {
                        serde_json::Number::from_f64(n)
                            .map(serde_json::Value::Number)
                            .unwrap_or_else(|| serde_json::Value::String(field.to_string()))
                    } else if field.to_lowercase() == "true" {
                        serde_json::Value::Bool(true)
                    } else if field.to_lowercase() == "false" {
                        serde_json::Value::Bool(false)
                    } else if field.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(field.to_string())
                    };
                    row.insert(headers[i].clone(), value);
                }
            }

            rows.push(row);
        }

        Ok(rows)
    }

    /// Read JSON data into rows
    pub fn read_json(
        data: &[u8],
        config: &JsonInput,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>, String> {
        let text = std::str::from_utf8(data).map_err(|e| format!("Invalid UTF-8: {}", e))?;

        if config.json_type.to_uppercase() == "DOCUMENT" {
            // Single JSON document (could be object or array)
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|e| format!("JSON parse error: {}", e))?;

            match value {
                serde_json::Value::Array(arr) => {
                    arr.into_iter()
                        .map(|v| {
                            if let serde_json::Value::Object(obj) = v {
                                Ok(obj.into_iter().collect())
                            } else {
                                // Wrap non-object values
                                let mut map = HashMap::new();
                                map.insert("_1".to_string(), v);
                                Ok(map)
                            }
                        })
                        .collect()
                }
                serde_json::Value::Object(obj) => Ok(vec![obj.into_iter().collect()]),
                _ => {
                    let mut map = HashMap::new();
                    map.insert("_1".to_string(), value);
                    Ok(vec![map])
                }
            }
        } else {
            // LINES - newline-delimited JSON
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    let value: serde_json::Value = serde_json::from_str(line)
                        .map_err(|e| format!("JSON parse error: {}", e))?;

                    if let serde_json::Value::Object(obj) = value {
                        Ok(obj.into_iter().collect())
                    } else {
                        let mut map = HashMap::new();
                        map.insert("_1".to_string(), value);
                        Ok(map)
                    }
                })
                .collect()
        }
    }

    /// Read Parquet data into rows
    ///
    /// Note: Full Parquet support requires additional integration with arrow2.
    /// For now, we return an error suggesting to use CSV or JSON format.
    /// This will be enhanced with RustySpark integration for GPU-accelerated Parquet reading.
    pub fn read_parquet(
        _data: &[u8],
    ) -> Result<Vec<HashMap<String, serde_json::Value>>, String> {
        // TODO: Integrate with RustySpark for full Parquet support
        // For now, suggest using CSV or JSON which are fully supported
        Err("Parquet input is not yet fully supported. Please use CSV or JSON format, or wait for RustySpark integration.".to_string())
    }
}

// =============================================================================
// Output Format Writers
// =============================================================================

#[cfg(feature = "s3-select")]
mod writers {
    use super::*;
    use std::collections::HashMap;

    /// Write rows as CSV
    pub fn write_csv(
        rows: &[HashMap<String, serde_json::Value>],
        config: &CsvOutput,
    ) -> Result<Vec<u8>, String> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let delimiter = config.field_delimiter.as_bytes().first().copied().unwrap_or(b',');
        let record_delimiter = &config.record_delimiter;

        // Get column names from first row
        let columns: Vec<&String> = rows[0].keys().collect();

        let mut output = Vec::new();

        for row in rows {
            let fields: Vec<String> = columns
                .iter()
                .map(|col| {
                    row.get(*col)
                        .map(|v| value_to_csv_field(v))
                        .unwrap_or_default()
                })
                .collect();

            output.extend_from_slice(fields.join(&String::from_utf8(vec![delimiter]).unwrap()).as_bytes());
            output.extend_from_slice(record_delimiter.as_bytes());
        }

        Ok(output)
    }

    fn value_to_csv_field(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => String::new(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => {
                // Quote if contains delimiter, quote, or newline
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s.clone()
                }
            }
            serde_json::Value::Array(arr) => serde_json::to_string(arr).unwrap_or_default(),
            serde_json::Value::Object(obj) => serde_json::to_string(obj).unwrap_or_default(),
        }
    }

    /// Write rows as JSON (newline-delimited)
    pub fn write_json(
        rows: &[HashMap<String, serde_json::Value>],
        config: &JsonOutput,
    ) -> Result<Vec<u8>, String> {
        let record_delimiter = &config.record_delimiter;

        let mut output = Vec::new();

        for row in rows {
            let json = serde_json::to_string(row).map_err(|e| format!("JSON write error: {}", e))?;
            output.extend_from_slice(json.as_bytes());
            output.extend_from_slice(record_delimiter.as_bytes());
        }

        Ok(output)
    }
}

// =============================================================================
// S3 Select Handler
// =============================================================================

/// Handle S3 Select request
#[cfg(feature = "s3-select")]
pub async fn select_object_content<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((bucket, key)): Path<(String, String)>,
    Query(_query): Query<SelectQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    // Parse the request body (XML or JSON)
    let request: SelectObjectContentRequest = parse_select_request(&body)?;

    // Validate expression type
    if request.expression_type.to_uppercase() != "SQL" {
        return Err(ApiError::InvalidRequest(
            "Only SQL expression type is supported".to_string(),
        ));
    }

    // Parse the SQL expression
    let parsed_query = evaluator::parse_sql(&request.expression)
        .map_err(ApiError::InvalidRequest)?;

    // Get the object data
    let object_key = ObjectKey::new(&bucket, &key)?;
    let data = state.store.get(&object_key).await?;
    let data_bytes = data.as_ref();

    // Apply scan range if specified
    let data_slice = if let Some(ref scan_range) = request.scan_range {
        let start = scan_range.start.unwrap_or(0) as usize;
        let end = scan_range.end.map(|e| e as usize).unwrap_or(data_bytes.len());
        &data_bytes[start.min(data_bytes.len())..end.min(data_bytes.len())]
    } else {
        data_bytes
    };

    // Read data based on input format
    let rows = if request.input_serialization.csv.is_some() {
        let config = request.input_serialization.csv.as_ref().unwrap();
        readers::read_csv(data_slice, config).map_err(ApiError::InvalidRequest)?
    } else if request.input_serialization.json.is_some() {
        let config = request.input_serialization.json.as_ref().unwrap();
        readers::read_json(data_slice, config).map_err(ApiError::InvalidRequest)?
    } else if request.input_serialization.parquet.is_some() {
        readers::read_parquet(data_slice).map_err(ApiError::InvalidRequest)?
    } else {
        return Err(ApiError::InvalidRequest(
            "No input serialization format specified".to_string(),
        ));
    };

    // Apply WHERE filter and projection
    let mut result_rows = Vec::new();
    let mut count = 0;

    for row in rows {
        // Check LIMIT
        if let Some(limit) = parsed_query.limit {
            if count >= limit {
                break;
            }
        }

        // Apply WHERE predicate
        let matches = match &parsed_query.predicate {
            Some(predicate) => evaluator::evaluate_predicate(&row, predicate),
            None => true,
        };

        if matches {
            // Apply projection
            let projected = evaluator::project_row(&row, &parsed_query.projections);
            result_rows.push(projected);
            count += 1;
        }
    }

    // Write output based on format
    let output = if request.output_serialization.csv.is_some() {
        let config = request.output_serialization.csv.as_ref().unwrap();
        writers::write_csv(&result_rows, config).map_err(ApiError::InvalidRequest)?
    } else if request.output_serialization.json.is_some() {
        let config = request.output_serialization.json.as_ref().unwrap();
        writers::write_json(&result_rows, config).map_err(ApiError::InvalidRequest)?
    } else {
        return Err(ApiError::InvalidRequest(
            "No output serialization format specified".to_string(),
        ));
    };

    // Return the output directly
    // For simplicity, we return plain data instead of the S3 event stream format
    // Full event stream format can be added later for streaming large results
    let content_type = if request.output_serialization.json.is_some() {
        "application/json"
    } else {
        "text/csv"
    };

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_LENGTH, output.len().to_string()),
        ],
        output,
    )
        .into_response())
}

/// Parse S3 Select request from XML or JSON body
#[cfg(feature = "s3-select")]
fn parse_select_request(body: &[u8]) -> ApiResult<SelectObjectContentRequest> {
    // Try JSON first (simpler for testing)
    if let Ok(request) = serde_json::from_slice::<SelectObjectContentRequest>(body) {
        return Ok(request);
    }

    // Try XML
    let text = std::str::from_utf8(body)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    // Use quick-xml for XML parsing
    quick_xml::de::from_str(text)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid request: {}", e)))
}


#[cfg(test)]
#[cfg(feature = "s3-select")]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let sql = "SELECT * FROM s3object";
        let parsed = evaluator::parse_sql(sql).unwrap();
        assert!(parsed.projections.is_none()); // SELECT *
        assert!(parsed.predicate.is_none());
        assert!(parsed.limit.is_none());
    }

    #[test]
    fn test_parse_select_with_where() {
        let sql = "SELECT name, age FROM s3object WHERE age > 25";
        let parsed = evaluator::parse_sql(sql).unwrap();
        assert!(parsed.projections.is_some());
        assert_eq!(parsed.projections.as_ref().unwrap().len(), 2);
        assert!(parsed.predicate.is_some());
    }

    #[test]
    fn test_parse_select_with_limit() {
        let sql = "SELECT * FROM s3object LIMIT 10";
        let parsed = evaluator::parse_sql(sql).unwrap();
        assert_eq!(parsed.limit, Some(10));
    }

    #[test]
    fn test_csv_reader() {
        let csv_data = b"name,age\nAlice,30\nBob,25";
        let config = CsvInput {
            file_header_info: "USE".to_string(),
            ..Default::default()
        };

        let rows = readers::read_csv(csv_data, &config).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("name").unwrap(), &serde_json::json!("Alice"));
        assert_eq!(rows[0].get("age").unwrap(), &serde_json::json!(30));
    }

    #[test]
    fn test_json_reader_lines() {
        let json_data = b"{\"name\":\"Alice\",\"age\":30}\n{\"name\":\"Bob\",\"age\":25}";
        let config = JsonInput {
            json_type: "LINES".to_string(),
        };

        let rows = readers::read_json(json_data, &config).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("name").unwrap(), &serde_json::json!("Alice"));
    }

    #[test]
    fn test_predicate_evaluation() {
        use std::collections::HashMap;

        let mut row: HashMap<String, serde_json::Value> = HashMap::new();
        row.insert("age".to_string(), serde_json::json!(30));
        row.insert("name".to_string(), serde_json::json!("Alice"));

        let sql = "SELECT * FROM s3object WHERE age > 25";
        let parsed = evaluator::parse_sql(sql).unwrap();

        let matches = evaluator::evaluate_predicate(&row, parsed.predicate.as_ref().unwrap());
        assert!(matches);
    }

    #[test]
    fn test_json_writer() {
        use std::collections::HashMap;

        let mut row: HashMap<String, serde_json::Value> = HashMap::new();
        row.insert("name".to_string(), serde_json::json!("Alice"));
        row.insert("age".to_string(), serde_json::json!(30));

        let config = JsonOutput {
            record_delimiter: "\n".to_string(),
        };

        let output = writers::write_json(&[row], &config).unwrap();
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("Alice"));
        assert!(output_str.contains("30"));
    }
}
