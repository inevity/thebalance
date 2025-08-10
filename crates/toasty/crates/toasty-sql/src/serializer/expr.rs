use super::{Comma, Delimited, Params, ToSql};

use crate::stmt;

impl ToSql for &stmt::Expr {
    fn to_sql<P: Params>(self, f: &mut super::Formatter<'_, P>) {
        use stmt::Expr::*;

        match self {
            And(expr) => {
                fmt!(f, Delimited(&expr.operands, " AND "));
            }
            BinaryOp(expr) => {
                assert!(!expr.lhs.is_value_null());
                assert!(!expr.rhs.is_value_null());

                fmt!(f, expr.lhs " " expr.op " " expr.rhs);
            }
            Column(stmt::ExprColumn::Column(column_id)) => {
                let column = f.serializer.column_name(*column_id);
                fmt!(f, column);
            }
            Column(stmt::ExprColumn::Alias {
                nesting, column, ..
            }) => {
                let depth = f.depth - *nesting;
                fmt!(f, "tbl_" depth ".col_" column)
            }
            Func(stmt::ExprFunc::Count(func)) => match (&func.arg, &func.filter) {
                (None, None) => fmt!(f, "COUNT(*)"),
                // Mysql does not support filters, so translate it to an expression
                (None, Some(expr)) if f.serializer.is_mysql() => {
                    fmt!(f, "COUNT(CASE WHEN " expr " THEN 1 END)")
                }
                (None, Some(expr)) => fmt!(f, "COUNT(*) FILTER (WHERE " expr ")"),
                _ => todo!("func={func:#?}"),
            },
            InList(expr) => {
                fmt!(f, expr.expr " IN " expr.list);
            }
            InSubquery(expr) => {
                fmt!(f, expr.expr " IN (" expr.query ")");
            }
            IsNull(expr) => {
                if expr.negate {
                    fmt!(f, expr.expr " IS NOT NULL");
                } else {
                    fmt!(f, expr.expr " IS NULL");
                }
            }
            Or(expr) => {
                fmt!(f, Delimited(&expr.operands, " OR "));
            }
            Pattern(stmt::ExprPattern::BeginsWith(expr)) => {
                let stmt::Expr::Value(pattern) = &*expr.pattern else {
                    todo!()
                };

                let pattern = pattern.expect_string();
                let pattern = format!("{pattern}%");
                let pattern = stmt::Expr::Value(pattern.into());

                fmt!(f, expr.expr " LIKE " pattern);
            }
            Record(expr) => {
                let exprs = Comma(&expr.fields);
                fmt!(f, "(" exprs ")");
            }
            Stmt(expr) => {
                let stmt = &*expr.stmt;
                fmt!(f, "(" stmt ")");
            }
            Value(expr) => expr.to_sql(f),
            Cast(expr) => {
                // For most casts, we can just serialize the inner expression
                // SQLite handles many implicit conversions
                match &expr.ty {
                    stmt::Type::String => {
                        // Explicit CAST for string conversion
                        fmt!(f, "CAST(" expr.expr " AS TEXT)");
                    }
                    stmt::Type::I32 | stmt::Type::I64 => {
                        // Explicit CAST for integer conversion
                        fmt!(f, "CAST(" expr.expr " AS INTEGER)");
                    }
                    stmt::Type::Bool => {
                        // SQLite doesn't have a native boolean type, use integer
                        fmt!(f, "CAST(" expr.expr " AS INTEGER)");
                    }
                    stmt::Type::Id(_) => {
                        // For ID types, just serialize the inner expression
                        // IDs are typically stored as strings in the database
                        expr.expr.to_sql(f);
                    }
                    _ => {
                        // For other types, serialize the inner expression
                        // and let the database handle implicit conversion
                        expr.expr.to_sql(f);
                    }
                }
            }
            List(expr) => {
                let exprs = Comma(&expr.items);
                fmt!(f, "(" exprs ")");
            }
            Field(expr) => {
                let name = &f.app_schema.field(expr.field).name;
                fmt!(f, name);
            }
            _ => todo!("expr={:?}", self),
        }
    }
}

impl ToSql for &stmt::BinaryOp {
    fn to_sql<P: Params>(self, f: &mut super::Formatter<'_, P>) {
        f.dst.push_str(match self {
            stmt::BinaryOp::Eq => "=",
            stmt::BinaryOp::Gt => ">",
            stmt::BinaryOp::Ge => ">=",
            stmt::BinaryOp::Lt => "<",
            stmt::BinaryOp::Le => "<=",
            stmt::BinaryOp::Ne => "<>",
            _ => todo!(),
        })
    }
}
