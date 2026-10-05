//! S2.4 contract tests for PlumbExpr language v1: lexis, literals, syntax, the normative
//! precedence and associativity table, the canonical unparser, the error contract, `Ty`,
//! round-trip and idempotence properties, no-panic behavior and the representative HR
//! expressions. Only syntax is tested: no test claims that an expression type-checks or
//! evaluates (S2.5).

use std::str::FromStr;

use plumb_core::{Id, Timestamp};
use plumb_expr::*;
use proptest::prelude::*;
use rust_decimal::Decimal;

// ---------------------------------------------------------------- helpers

fn p(input: &str) -> Ast {
    parse(input).unwrap_or_else(|e| panic!("{input:?}: {e:?}"))
}

fn canon(input: &str) -> String {
    canonicalize(input).unwrap_or_else(|e| panic!("{input:?}: {e:?}"))
}

fn ident(s: &str) -> Ast {
    Ast::Identifier(Identifier::from_str(s).unwrap())
}

fn int(v: i64) -> Ast {
    Ast::Literal(Literal::Int(v))
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn bin(op: BinaryOp, left: Ast, right: Ast) -> Ast {
    Ast::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn un(op: UnaryOp, expr: Ast) -> Ast {
    Ast::Unary {
        op,
        expr: Box::new(expr),
    }
}

fn call(f: &str, args: Vec<Ast>) -> Ast {
    Ast::Call {
        function: Symbol::new(f).unwrap(),
        args,
    }
}

fn cond(c: Ast, t: Ast, e: Ast) -> Ast {
    Ast::Conditional {
        condition: Box::new(c),
        then_expr: Box::new(t),
        else_expr: Box::new(e),
    }
}

fn a() -> Ast {
    ident("a")
}
fn b() -> Ast {
    ident("b")
}
fn c() -> Ast {
    ident("c")
}
fn d() -> Ast {
    ident("d")
}

use BinaryOp::*;

fn rejects(input: &str) {
    assert!(parse(input).is_err(), "{input:?} must be rejected");
}

// ---------------------------------------------------------------- version, words, identifiers

#[test]
fn parser_language_version_and_reserved_words() {
    assert_eq!(PLUMB_EXPR_LANGUAGE_VERSION, 1);
    assert_eq!(
        RESERVED_WORDS,
        [
            "true", "false", "and", "or", "not", "in", "if", "then", "else", "date", "datetime",
            "duration", "quantity"
        ]
    );
    for word in RESERVED_WORDS {
        assert!(Symbol::new(word).is_err(), "{word}");
        // Reserved words cannot be identifiers or function names.
        if !["true", "false"].contains(&word) {
            rejects(word);
        }
        if word != "not" {
            rejects(&format!("{word}(x)"));
        }
    }
    // Case-sensitive keywords and token boundaries.
    for symbol in [
        "Annual",
        "Date",
        "If",
        "TRUE",
        "index",
        "android",
        "notified",
        "dates",
        "iffy",
        "orbit",
        "thence",
        "elsewhere",
    ] {
        assert_eq!(p(symbol), ident(symbol), "{symbol}");
    }
}

#[test]
fn parser_identifiers() {
    for s in [
        "employee",
        "LeaveRequest.start_date",
        "_requested",
        "x2",
        "Annual",
        "_derived",
        "a.b.c",
    ] {
        assert_eq!(p(s), ident(s));
        assert_eq!(canon(s), s);
    }
    let path = Identifier::from_str("LeaveRequest.start_date").unwrap();
    assert_eq!(
        path.segments()
            .iter()
            .map(Symbol::as_str)
            .collect::<Vec<_>>(),
        ["LeaveRequest", "start_date"]
    );
    assert_eq!(path.to_string(), "LeaveRequest.start_date");
    for s in [
        "2x",
        "employee id",
        "a..b",
        ".a",
        "a.",
        "foo::bar",
        "a. b",
        "a .b",
    ] {
        rejects(s);
    }
    // As expressions these are operators over identifiers, not identifiers.
    assert_eq!(
        p("employee-id"),
        bin(Subtract, ident("employee"), ident("id"))
    );
    assert_eq!(p("foo/bar"), bin(Divide, ident("foo"), ident("bar")));
    // A reserved word used as a prefix operator is still the operator.
    assert_eq!(p("not(x)"), un(UnaryOp::Not, ident("x")));
    for s in ["2x", "employee-id", "", "a b", "if"] {
        assert!(Symbol::new(s).is_err(), "{s}");
    }
    for s in ["a..b", ".a", "a.", "", "a.if"] {
        assert!(Identifier::from_str(s).is_err(), "{s}");
    }
    assert_eq!(Symbol::new("x2").unwrap().to_string(), "x2");
    // Whitespace is insignificant between tokens; whitespace-only input is Empty.
    assert_eq!(p(" \t\r\n a \n"), a());
    for empty in ["", " ", "\t\r\n"] {
        assert_eq!(parse(empty), Err(ParseError::Empty));
    }
    // No comments exist.
    rejects("a // c");
    rejects("a # c");
    rejects("a /* c */");
}

// ---------------------------------------------------------------- literals

#[test]
fn parser_numeric_literals() {
    assert_eq!(p("0"), int(0));
    assert_eq!(p("1"), int(1));
    assert_eq!(p("42"), int(42));
    assert_eq!(p("-1"), un(UnaryOp::Negate, int(1)));
    assert_eq!(p("9223372036854775807"), int(i64::MAX));
    assert_eq!(
        parse("9223372036854775808"),
        Err(ParseError::InvalidInteger { offset: 0 })
    );
    for (s, value) in [
        ("0.5", "0.5"),
        ("1.0", "1.0"),
        ("20.00", "20.00"),
        ("123.456", "123.456"),
    ] {
        let Ast::Literal(Literal::Decimal(d)) = p(s) else {
            panic!("{s}")
        };
        assert_eq!(d.to_string(), value);
        assert_eq!(canon(s), value, "scale preserved");
    }
    let Ast::Literal(Literal::Decimal(d)) = p("1.00") else {
        panic!()
    };
    assert_eq!(d.scale(), 2);
    assert_eq!(
        p("-0.5"),
        un(UnaryOp::Negate, Ast::Literal(Literal::Decimal(dec("0.5"))))
    );
    assert_eq!(parse("00"), Err(ParseError::InvalidInteger { offset: 0 }));
    assert_eq!(parse("01"), Err(ParseError::InvalidInteger { offset: 0 }));
    assert_eq!(parse("007"), Err(ParseError::InvalidInteger { offset: 0 }));
    assert_eq!(parse("1."), Err(ParseError::InvalidDecimal { offset: 0 }));
    assert_eq!(parse("01.5"), Err(ParseError::InvalidDecimal { offset: 0 }));
    assert_eq!(
        parse("a + 01.5"),
        Err(ParseError::InvalidDecimal { offset: 4 })
    );
    // Beyond rust_decimal's exact range: rejected rather than rounded.
    assert!(matches!(
        parse("0.00000000000000000000000000001"),
        Err(ParseError::InvalidDecimal { .. })
    ));
    for s in [".5", "1e3", "1E3", "1_000", "1_000.0", "0x10", "1L", "+1"] {
        assert!(parse(s).is_err(), "{s}");
    }
    // NaN and Infinity are ordinary identifiers, never numeric literals.
    assert_eq!(p("NaN"), ident("NaN"));
    assert_eq!(p("Infinity"), ident("Infinity"));
}

#[test]
fn parser_bool_and_string_literals() {
    assert_eq!(p("true"), Ast::Literal(Literal::Bool(true)));
    assert_eq!(p("false"), Ast::Literal(Literal::Bool(false)));
    for alias in ["True", "FALSE", "yes", "no"] {
        assert!(
            matches!(p(alias), Ast::Identifier(_)),
            "{alias} is not a boolean"
        );
    }
    let s = |v: &str| Ast::Literal(Literal::String(v.to_owned()));
    assert_eq!(p(r#""""#), s(""));
    assert_eq!(p(r#""hello""#), s("hello"));
    assert_eq!(p(r#""hello world""#), s("hello world"));
    assert_eq!(p(r#""quote: \"""#), s("quote: \""));
    assert_eq!(p(r#""slash: \\""#), s("slash: \\"));
    assert_eq!(p(r#""line\nbreak""#), s("line\nbreak"));
    assert_eq!(p(r#""\r\t\b\f""#), s("\r\t\u{8}\u{c}"));
    assert_eq!(p("\"zażółć 日本 ✓\""), s("zażółć 日本 ✓"));
    assert_eq!(p(r#""^ and ** inside""#), s("^ and ** inside"));
    assert_eq!(
        canon(r#""a\"b\\c\nd\re\tf\bg\fh""#),
        r#""a\"b\\c\nd\re\tf\bg\fh""#
    );
    assert_eq!(canon("\"é\""), "\"é\"");
    assert!(parse("'single'").is_err());
    assert_eq!(
        parse(r#""\q""#),
        Err(ParseError::InvalidString { offset: 1 })
    );
    assert_eq!(
        parse(r#""ab\u0041""#),
        Err(ParseError::InvalidString { offset: 3 })
    );
    assert_eq!(
        parse("\"a\u{1}b\""),
        Err(ParseError::InvalidString { offset: 2 })
    );
    assert_eq!(
        parse("\"a\nb\""),
        Err(ParseError::InvalidString { offset: 2 })
    );
    assert_eq!(
        parse("\"é\tx\""),
        Err(ParseError::InvalidString { offset: 3 }),
        "byte offset after a 2-byte char"
    );
    assert!(matches!(
        parse("\"unterminated"),
        Err(ParseError::Syntax { .. })
    ));
    // No interpolation: braces are ordinary characters.
    assert_eq!(p(r#""${x}""#), s("${x}"));
}

#[test]
fn parser_date_and_datetime_literals() {
    let date = |y, m, d| {
        Ast::Literal(Literal::Date(
            time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap(),
        ))
    };
    assert_eq!(p(r#"date("2027-02-01")"#), date(2027, 2, 1));
    assert_eq!(p(r#"date("2028-02-29")"#), date(2028, 2, 29));
    assert_eq!(p(r#"date("2000-02-29")"#), date(2000, 2, 29));
    assert_eq!(p(r#"date("0000-01-01")"#), date(0, 1, 1));
    assert_eq!(p(r#"date("9999-12-31")"#), date(9999, 12, 31));
    assert_eq!(canon(r#"date( "2027-02-01" )"#), r#"date("2027-02-01")"#);
    for bad in [
        "2027-02-30",
        "2027-02-29",
        "1900-02-29",
        "2027-13-01",
        "2027-00-10",
        "2027-04-31",
        "2027-2-1",
        "27-02-01",
        "2027-02-01T00:00:00Z",
        "",
    ] {
        assert_eq!(
            parse(&format!(r#"date("{bad}")"#)),
            Err(ParseError::InvalidDate { offset: 5 }),
            "{bad}"
        );
    }
    // DateTime reuses plumb_core::Timestamp: offsets normalize to UTC.
    let plus = p(r#"datetime("2027-01-20T10:00:00+01:00")"#);
    let zulu = p(r#"datetime("2027-01-20T09:00:00Z")"#);
    assert_eq!(plus, zulu);
    assert_eq!(
        plus,
        Ast::Literal(Literal::DateTime(
            Timestamp::from_str("2027-01-20T09:00:00Z").unwrap()
        ))
    );
    for source in [
        r#"datetime("2027-01-20T10:00:00+01:00")"#,
        r#"datetime("2027-01-20T09:00:00Z")"#,
    ] {
        assert_eq!(
            canon(source),
            r#"datetime("2027-01-20T09:00:00.000000000Z")"#
        );
    }
    for bad in [
        "2027-01-20",
        "2027-01-20T25:00:00Z",
        "not a time",
        " 2027-01-20T09:00:00Z",
        "10000-01-01T00:00:00Z",
    ] {
        assert_eq!(
            parse(&format!(r#"datetime("{bad}")"#)),
            Err(ParseError::InvalidDateTime { offset: 9 }),
            "{bad}"
        );
    }
}

#[test]
fn parser_units_durations_and_quantities() {
    for unit in ["day", "hour", "second", "working_day", "currency_eur", "x1"] {
        assert_eq!(Unit::new(unit).unwrap().as_str(), unit);
        assert_eq!(Unit::from_str(unit).unwrap().to_string(), unit);
    }
    for unit in [
        "WorkingDay",
        "working-day",
        "working day",
        "_day",
        "1day",
        "",
        "Day",
    ] {
        assert!(Unit::new(unit).is_err(), "{unit}");
    }
    let lit = |is_duration: bool, v: &str, u: &str| {
        let (value, unit) = (dec(v), Unit::new(u).unwrap());
        Ast::Literal(if is_duration {
            Literal::Duration { value, unit }
        } else {
            Literal::Quantity { value, unit }
        })
    };
    assert_eq!(p("duration(5, day)"), lit(true, "5", "day"));
    assert_eq!(p("duration(2.5, hour)"), lit(true, "2.5", "hour"));
    assert_eq!(
        p("quantity(0.5, working_day)"),
        lit(false, "0.5", "working_day")
    );
    assert_eq!(p("quantity(2, second)"), lit(false, "2", "second"));
    assert_eq!(canon("duration( 5 ,day )"), "duration(5, day)");
    assert_eq!(canon("duration(2.50, hour)"), "duration(2.50, hour)");
    assert_eq!(
        canon("quantity(0.50, working_day)"),
        "quantity(0.50, working_day)"
    );
    assert_eq!(
        p("-duration(5, day)"),
        un(UnaryOp::Negate, lit(true, "5", "day"))
    );
    assert_eq!(
        p("-quantity(0.5, working_day)"),
        un(UnaryOp::Negate, lit(false, "0.5", "working_day"))
    );
    for bad in [
        "duration(-5, day)",
        "quantity(-0.5, working_day)",
        "duration(5)",
        "duration(5, working day)",
        "duration(5, working-day)",
        "duration(day, 5)",
        "duration(\"P5D\")",
    ] {
        assert!(
            matches!(parse(bad), Err(ParseError::Syntax { .. })),
            "{bad}"
        );
    }
    assert_eq!(
        parse("duration(5, WorkingDay)"),
        Err(ParseError::InvalidUnit { offset: 12 })
    );
    assert_eq!(
        parse("quantity(1, _day)"),
        Err(ParseError::InvalidUnit { offset: 12 })
    );
    assert_eq!(
        parse("quantity(1, 1day)"),
        Err(ParseError::InvalidUnit { offset: 12 })
    );
    assert_eq!(
        parse("duration(01, day)"),
        Err(ParseError::InvalidInteger { offset: 9 })
    );
    assert_eq!(
        parse("duration(1., day)"),
        Err(ParseError::InvalidDecimal { offset: 9 })
    );
}

// ---------------------------------------------------------------- syntax

#[test]
fn parser_lists_and_calls() {
    assert_eq!(p("[]"), Ast::List(vec![]));
    assert_eq!(p("[a]"), Ast::List(vec![a()]));
    assert_eq!(p("[a, b]"), Ast::List(vec![a(), b()]));
    assert_eq!(
        p("[start_date, end_date]"),
        Ast::List(vec![ident("start_date"), ident("end_date")])
    );
    assert_eq!(canon("[ a ,b ]"), "[a, b]");
    rejects("[a,]");
    rejects("[a, b,]");
    rejects("[,]");
    assert_eq!(p("f()"), call("f", vec![]));
    assert_eq!(p("f(a)"), call("f", vec![a()]));
    assert_eq!(p("f(a, b)"), call("f", vec![a(), b()]));
    assert_eq!(
        p("exists(ApprovalRecord)"),
        call("exists", vec![ident("ApprovalRecord")])
    );
    assert_eq!(
        p("working_days([start_date, end_date], calendar, inclusive_end)"),
        call(
            "working_days",
            vec![
                Ast::List(vec![ident("start_date"), ident("end_date")]),
                ident("calendar"),
                ident("inclusive_end"),
            ]
        )
    );
    assert_eq!(canon("f( a,b )"), "f(a, b)");
    // Any syntactically valid symbol parses as a call; nothing is resolved or executed.
    assert_eq!(
        p("no_overlap(Approved, LeaveRequest, employee)"),
        call(
            "no_overlap",
            vec![ident("Approved"), ident("LeaveRequest"), ident("employee")]
        )
    );
    assert_eq!(p("system(rm)"), call("system", vec![ident("rm")]));
    for bad in ["a.b()", "a.b.c()", "(a)(b)", "f(a,)", "f(a)(b)", "f()()"] {
        rejects(bad);
    }
}

#[test]
fn parser_operators_and_rejections() {
    for (src, op) in [
        ("a + b", Add),
        ("a - b", Subtract),
        ("a * b", Multiply),
        ("a / b", Divide),
        ("a % b", Remainder),
        ("a == b", Equal),
        ("a != b", NotEqual),
        ("a < b", LessThan),
        ("a <= b", LessThanOrEqual),
        ("a > b", GreaterThan),
        ("a >= b", GreaterThanOrEqual),
        ("a in b", In),
        ("a and b", And),
        ("a or b", Or),
        ("a -> b", Implies),
    ] {
        assert_eq!(p(src), bin(op, a(), b()), "{src}");
        assert_eq!(canon(src), src);
        assert_eq!(op.token(), src.split(' ').nth(1).unwrap());
    }
    assert_eq!(BinaryOp::ALL.len(), 15);
    assert_eq!(p("-a"), un(UnaryOp::Negate, a()));
    assert_eq!(p("--a"), un(UnaryOp::Negate, un(UnaryOp::Negate, a())));
    assert_eq!(p("not active"), un(UnaryOp::Not, ident("active")));
    assert_eq!(
        p("not not active"),
        un(UnaryOp::Not, un(UnaryOp::Not, ident("active")))
    );
    for bad in [
        "a = b",
        "a <> b",
        "a === b",
        "a && b",
        "a || b",
        "!a",
        "a ** b",
        "a ^ b",
        "+a",
        "~a",
        "a not in b",
        "a implies b",
        "a ? b : c",
        "a; b",
        "x = 1",
        "let x = 1",
        "a[0]",
    ] {
        rejects(bad);
    }
    // Chained comparisons are rejected; explicit grouping is syntax.
    for bad in ["a < b < c", "a == b == c", "a in b in c", "a <= b > c"] {
        assert!(
            matches!(parse(bad), Err(ParseError::Syntax { .. })),
            "{bad}"
        );
    }
    assert_eq!(parse("a < b < c"), Err(ParseError::Syntax { offset: 6 }));
    assert_eq!(
        p("(a < b) < c"),
        bin(LessThan, bin(LessThan, a(), b()), c())
    );
}

#[test]
fn parser_precedence_table() {
    assert_eq!(p("a + b * c"), bin(Add, a(), bin(Multiply, b(), c())));
    assert_eq!(p("(a + b) * c"), bin(Multiply, bin(Add, a(), b()), c()));
    assert_eq!(p("a * b + c"), bin(Add, bin(Multiply, a(), b()), c()));
    assert_eq!(p("-a * b"), bin(Multiply, un(UnaryOp::Negate, a()), b()));
    assert_eq!(p("not a and b"), bin(And, un(UnaryOp::Not, a()), b()));
    assert_eq!(
        p("a == b and c == d"),
        bin(And, bin(Equal, a(), b()), bin(Equal, c(), d()))
    );
    assert_eq!(p("a or b and c"), bin(Or, a(), bin(And, b(), c())));
    assert_eq!(
        p("a + b < c * d"),
        bin(LessThan, bin(Add, a(), b()), bin(Multiply, c(), d()))
    );
    assert_eq!(
        p("x in [a, b]"),
        bin(In, ident("x"), Ast::List(vec![a(), b()]))
    );
    assert_eq!(p("a -> b -> c"), bin(Implies, a(), bin(Implies, b(), c())));
    assert_eq!(
        p("(a -> b) -> c"),
        bin(Implies, bin(Implies, a(), b()), c())
    );
    assert_eq!(p("a or b -> c"), bin(Implies, bin(Or, a(), b()), c()));
    assert_eq!(p("if a then b else c"), cond(a(), b(), c()));
    assert_eq!(
        p("if a then b else if c then d else e"),
        cond(a(), b(), cond(c(), d(), ident("e")))
    );
    assert_eq!(
        p("if a then b else c -> d"),
        cond(a(), b(), bin(Implies, c(), d()))
    );
    assert_eq!(p("a - b - c"), bin(Subtract, bin(Subtract, a(), b()), c()));
    assert_eq!(p("a / b % c"), bin(Remainder, bin(Divide, a(), b()), c()));
    assert_eq!(p("a and b and c"), bin(And, bin(And, a(), b()), c()));
    assert_eq!(p("a or b or c"), bin(Or, bin(Or, a(), b()), c()));
    assert_eq!(p("not a == b"), bin(Equal, un(UnaryOp::Not, a()), b()));
    assert_eq!(p("a - -b"), bin(Subtract, a(), un(UnaryOp::Negate, b())));
    // Parentheses create no node.
    assert_eq!(p("((a))"), a());
    assert_eq!(
        p("not (a in [b, c])"),
        un(UnaryOp::Not, bin(In, a(), Ast::List(vec![b(), c()])))
    );
    // A conditional is an operand only when parenthesized.
    assert_eq!(
        p("(if a then b else c) + d"),
        bin(Add, cond(a(), b(), c()), d())
    );
    rejects("a + if b then c else d");
    rejects("if a then b");
}

#[test]
fn parser_canonical_unparse() {
    for (source, canonical) in [
        ("a+b*c", "a + b * c"),
        ("(a+b)*c", "(a + b) * c"),
        ("((a))", "a"),
        ("a - (b - c)", "a - (b - c)"),
        ("(a - b) - c", "a - b - c"),
        ("a -> (b -> c)", "a -> b -> c"),
        ("(a -> b) -> c", "(a -> b) -> c"),
        ("(a < b) < c", "(a < b) < c"),
        ("a < (b < c)", "a < (b < c)"),
        ("- ( a + b )", "-(a + b)"),
        ("not(a)", "not a"),
        ("not (not a)", "not not a"),
        ("-(-a)", "--a"),
        ("(if a then b else c) -> d", "(if a then b else c) -> d"),
        ("a -> (if b then c else d)", "a -> (if b then c else d)"),
        (
            "if (if a then b else c) then d else e",
            "if if a then b else c then d else e",
        ),
        (
            "if a then (if b then c else d) else e",
            "if a then if b then c else d else e",
        ),
        ("if a then b else (c or d)", "if a then b else c or d"),
        ("f( [a,b] , g( ) )", "f([a, b], g())"),
        ("a.b.c  ==  1.50", "a.b.c == 1.50"),
        ("(a and b) or c", "a and b or c"),
        ("a and (b or c)", "a and (b or c)"),
        ("(-a) * b", "-a * b"),
        ("-(a * b)", "-(a * b)"),
    ] {
        assert_eq!(canon(source), canonical, "{source}");
        assert_eq!(canon(canonical), canonical, "idempotent {canonical}");
        assert_eq!(p(canonical), p(source));
    }
}

// ---------------------------------------------------------------- HR expressions

#[test]
fn parser_hr_representative_expressions() {
    // Syntax only: no claim that these type-check or evaluate (S2.5).
    let cases = [
        (
            "LeaveRequest.start_date <= LeaveRequest.end_date",
            bin(LessThanOrEqual, ident("LeaveRequest.start_date"), ident("LeaveRequest.end_date")),
        ),
        ("LeaveBalance.remaining_days >= 0", bin(GreaterThanOrEqual, ident("LeaveBalance.remaining_days"), int(0))),
        (
            "LeaveRequest.status == Approved -> exists(ApprovalRecord)",
            bin(
                Implies,
                bin(Equal, ident("LeaveRequest.status"), ident("Approved")),
                call("exists", vec![ident("ApprovalRecord")]),
            ),
        ),
        (
            "ApproveLeaveRequest.actor != LeaveRequest.employee and direct_manager(actor, employee)",
            bin(
                And,
                bin(NotEqual, ident("ApproveLeaveRequest.actor"), ident("LeaveRequest.employee")),
                call("direct_manager", vec![ident("actor"), ident("employee")]),
            ),
        ),
        (
            "working_days([start_date, end_date], calendar, inclusive_end) * day_fraction",
            bin(
                Multiply,
                call(
                    "working_days",
                    vec![Ast::List(vec![ident("start_date"), ident("end_date")]), ident("calendar"), ident("inclusive_end")],
                ),
                ident("day_fraction"),
            ),
        ),
        (
            "if leave_type == Annual then RequestedWorkingDays else 0",
            cond(bin(Equal, ident("leave_type"), ident("Annual")), ident("RequestedWorkingDays"), int(0)),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(p(source), expected, "{source}");
        assert_eq!(canon(source), source, "already canonical");
    }
    // The fixture's no_overlap line is descriptive prose, not a frozen lexical form.
    rejects("no_overlap(Approved LeaveRequest for same employee)");
    assert!(matches!(
        p("no_overlap(Approved, LeaveRequest, employee)"),
        Ast::Call { .. }
    ));
}

// ---------------------------------------------------------------- errors

#[test]
fn parser_error_contract() {
    assert_eq!(ParseError::Empty.offset(), None);
    assert_eq!(parse("a +"), Err(ParseError::Syntax { offset: 3 }));
    assert_eq!(parse("a b"), Err(ParseError::Syntax { offset: 2 }));
    assert_eq!(parse("(a"), Err(ParseError::Syntax { offset: 2 }));
    // Byte offsets, not character indices: "é" is two bytes.
    assert_eq!(parse("\"é\" ?"), Err(ParseError::Syntax { offset: 5 }));
    assert_eq!(parse("é"), Err(ParseError::Syntax { offset: 0 }));
    assert_eq!(
        parse("[\"é\", 01]"),
        Err(ParseError::InvalidInteger { offset: 7 })
    );
    let classes = [
        parse(""),
        parse("a +"),
        parse("01"),
        parse("01.5"),
        parse(r#""\q""#),
        parse(r#"date("2027-02-30")"#),
        parse(r#"datetime("x")"#),
        parse("duration(1, X)"),
    ];
    let names: Vec<String> = classes
        .iter()
        .map(|r| {
            format!("{:?}", r.as_ref().unwrap_err())
                .split([' ', '{'])
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        names,
        [
            "Empty",
            "Syntax",
            "InvalidInteger",
            "InvalidDecimal",
            "InvalidString",
            "InvalidDate",
            "InvalidDateTime",
            "InvalidUnit"
        ]
    );
}

// ---------------------------------------------------------------- Ty

#[test]
fn parser_ty_variants() {
    let id = |s: &str| Id::from_str(s).unwrap();
    let unit = Unit::new("working_day").unwrap();
    let valid = [
        Ty::Int,
        Ty::Decimal(0),
        Ty::Decimal(2),
        Ty::Decimal(28),
        Ty::Bool,
        Ty::String,
        Ty::Date,
        Ty::DateTime,
        Ty::Duration(unit.clone()),
        Ty::Quantity(unit.clone()),
        Ty::Enum(id("enum:leave_type")),
        Ty::Ref(id("entity:employee")),
        Ty::List(Box::new(Ty::String)),
        Ty::List(Box::new(Ty::List(Box::new(Ty::Decimal(2))))),
    ];
    for ty in &valid {
        assert_eq!(ty.validate(), Ok(()), "{ty:?}");
    }
    assert_eq!(MAX_DECIMAL_SCALE, 28);
    assert_eq!(
        Ty::Decimal(29).validate(),
        Err(TypeError::InvalidDecimalScale(29))
    );
    assert_eq!(
        Ty::List(Box::new(Ty::Decimal(29))).validate(),
        Err(TypeError::InvalidDecimalScale(29))
    );
    assert_ne!(
        Ty::Duration(unit.clone()),
        Ty::Duration(Unit::new("day").unwrap())
    );
    assert_ne!(Ty::Duration(unit.clone()), Ty::Quantity(unit));
}

// ---------------------------------------------------------------- guards

#[test]
fn parser_source_guard() {
    let sources = [
        ("lib.rs", include_str!("../src/lib.rs")),
        ("ast.rs", include_str!("../src/ast.rs")),
        ("types.rs", include_str!("../src/types.rs")),
        ("parser.rs", include_str!("../src/parser.rs")),
        ("grammar.pest", include_str!("../src/grammar.pest")),
    ];
    for (name, source) in sources {
        for forbidden in [
            "std::fs",
            "File::open",
            "std::process",
            "Command",
            "reqwest",
            "TcpStream",
            "Clock",
            "Utc::now",
            "now_utc",
            "Instant::now",
            "ArtifactStore",
            "RevisionStore",
            "Sqlite",
            "Graph",
            "unsafe",
            "eval(",
            "libloading",
            "std::env",
            "plumb_psg",
            "plumb_functional",
            "plumb_validation",
            "plumb_inference",
            "plumb_patch",
            "plumb_store",
            "typecheck",
            "TypeEnv",
            "CalendarProvider",
            "working_days(",
            "LeaveRequest",
            "ApprovalRecord",
            "Approved",
            "Annual",
        ] {
            assert!(!source.contains(forbidden), "{name} contains {forbidden}");
        }
    }
    // The pest parser type is internal.
    assert!(!include_str!("../src/lib.rs").contains("PlumbExprGrammar"));
    assert!(!include_str!("../src/lib.rs").contains("pest::"));
}

// ---------------------------------------------------------------- properties

fn symbol_strategy() -> impl Strategy<Value = Symbol> {
    "[A-Za-z_][A-Za-z0-9_]{0,6}".prop_filter_map("reserved", |s| Symbol::new(&s).ok())
}

fn unit_strategy() -> impl Strategy<Value = Unit> {
    "[a-z][a-z0-9_]{0,6}".prop_map(|s| Unit::new(&s).unwrap())
}

fn decimal_strategy(min_scale: u32) -> impl Strategy<Value = Decimal> {
    (0i64..1_000_000_000_000, min_scale..=6u32).prop_map(|(m, s)| Decimal::new(m, s))
}

fn literal_strategy() -> impl Strategy<Value = Literal> {
    prop_oneof![
        (0i64..=i64::MAX).prop_map(Literal::Int),
        decimal_strategy(1).prop_map(Literal::Decimal),
        any::<bool>().prop_map(Literal::Bool),
        proptest::collection::vec(
            prop_oneof![
                proptest::char::range('a', 'z'),
                Just('"'),
                Just('\\'),
                Just('\n'),
                Just('\r'),
                Just('\t'),
                Just('\u{8}'),
                Just('\u{c}'),
                Just(' '),
                Just('é'),
                Just('日'),
                Just('{'),
            ],
            0..8
        )
        .prop_map(|chars| Literal::String(chars.into_iter().collect())),
        (0i32..=9999, 1u8..=12, 1u8..=28).prop_map(|(y, m, d)| Literal::Date(
            time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
        )),
        (-62_000_000_000i64..253_000_000_000i64, 0u32..1_000_000_000).prop_map(|(s, n)| {
            let at = time::OffsetDateTime::from_unix_timestamp(s).unwrap()
                + time::Duration::nanoseconds(i64::from(n));
            Literal::DateTime(Timestamp::try_from(at).unwrap())
        }),
        (decimal_strategy(0), unit_strategy())
            .prop_map(|(value, unit)| Literal::Duration { value, unit }),
        (decimal_strategy(0), unit_strategy())
            .prop_map(|(value, unit)| Literal::Quantity { value, unit }),
    ]
}

fn ast_strategy() -> impl Strategy<Value = Ast> {
    let leaf = prop_oneof![
        literal_strategy().prop_map(Ast::Literal),
        proptest::collection::vec(symbol_strategy(), 1..4)
            .prop_map(|s| Ast::Identifier(Identifier::new(s).unwrap())),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Ast::List),
            (
                symbol_strategy(),
                proptest::collection::vec(inner.clone(), 0..4)
            )
                .prop_map(|(function, args)| Ast::Call { function, args }),
            (
                prop_oneof![Just(UnaryOp::Negate), Just(UnaryOp::Not)],
                inner.clone()
            )
                .prop_map(|(op, expr)| Ast::Unary {
                    op,
                    expr: Box::new(expr)
                }),
            (
                proptest::sample::select(BinaryOp::ALL.to_vec()),
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(op, l, r)| Ast::Binary {
                    op,
                    left: Box::new(l),
                    right: Box::new(r)
                }),
            (inner.clone(), inner.clone(), inner).prop_map(|(c, t, e)| Ast::Conditional {
                condition: Box::new(c),
                then_expr: Box::new(t),
                else_expr: Box::new(e),
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn parser_property_parse_unparse_round_trip(ast in ast_strategy()) {
        let text = unparse(&ast);
        let reparsed = parse(&text);
        prop_assert_eq!(reparsed.as_ref(), Ok(&ast), "{}", text);
        // Canonicalization is idempotent and preserves decimal scale textually.
        let again = unparse(reparsed.as_ref().unwrap());
        prop_assert_eq!(&again, &text);
        prop_assert_eq!(canonicalize(&again), Ok(text));
    }

    #[test]
    fn parser_property_canonicalization_idempotent(
        tokens in proptest::collection::vec(
            proptest::sample::select(vec![
                "a", "b.c", "1", "2.50", "(", ")", "+", "-", "*", "/", "%", "==", "!=", "<", "<=",
                "in", "and", "or", "not", "->", "if", "then", "else", "[", "]", ",", "f(", "\"x\"",
                "date(\"2027-02-01\")", "duration(5, day)", " ", "true",
            ]),
            1..14,
        )
    ) {
        let input = tokens.join(" ");
        if let Ok(ast) = parse(&input) {
            let c1 = unparse(&ast);
            let c2 = unparse(&parse(&c1).unwrap());
            prop_assert_eq!(c1, c2);
        }
    }

    #[test]
    fn parser_property_arbitrary_utf8_never_panics(input in any::<String>()) {
        let _ = parse(&input);
        let _ = canonicalize(&input);
    }

    #[test]
    fn parser_property_error_offsets_are_char_boundaries(input in "[ -~éł日]{0,24}") {
        if let Err(e) = parse(&input) {
            if let Some(offset) = e.offset() {
                prop_assert!(offset <= input.len());
                prop_assert!(input.is_char_boundary(offset));
            }
        }
    }
}
