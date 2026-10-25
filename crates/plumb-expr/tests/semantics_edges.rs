//! E0.4 mutation-gap tests for PlumbExpr: the equality, ordering, arithmetic and date-shift
//! semantics of every value kind, the typing rules whose rejections the S2.5 suites did not
//! pin, the idempotent environment rebinding and the nesting pre-scan boundaries. Each test
//! asserts the specified result, not merely that the code path runs.

use std::str::FromStr;

use plumb_core::{Id, Timestamp};
use plumb_expr::*;
use rust_decimal::Decimal;
use time::{Date, Month};

fn id(s: &str) -> Id {
    Id::from_str(s).unwrap()
}

fn sym(s: &str) -> Symbol {
    Symbol::new(s).unwrap()
}

fn u(s: &str) -> Unit {
    Unit::new(s).unwrap()
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn date(y: i32, m: u8, d: u8) -> Date {
    Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap()
}

const STATUS: &str = "attr:Request-status";

struct NoManagers;

impl PredicateProvider for NoManagers {
    fn direct_manager(&self, _: &RefValue, _: &RefValue) -> Result<bool, PredicateError> {
        Ok(false)
    }
}

/// Scalars, unit-bearing values, dates and an enum, all bound.
fn env() -> (TypeEnv, ValueEnv) {
    let mut types = TypeEnv::new();
    let mut values = ValueEnv::new();
    let status = Value::Enum {
        enum_ref: id(STATUS),
        member: sym("Approved"),
    };
    for (name, ty, value) in [
        ("i", Ty::Int, Value::Int(7)),
        ("j", Ty::Int, Value::Int(-3)),
        ("d", Ty::Decimal(2), Value::Decimal(dec("1.25"))),
        ("b", Ty::Bool, Value::Bool(true)),
        ("f", Ty::Bool, Value::Bool(false)),
        ("s", Ty::String, Value::String("x".into())),
        (
            "h1",
            Ty::Quantity(u("hour")),
            Value::Quantity {
                value: dec("1.5"),
                unit: u("hour"),
            },
        ),
        (
            "h2",
            Ty::Quantity(u("hour")),
            Value::Quantity {
                value: dec("2"),
                unit: u("hour"),
            },
        ),
        (
            "dur",
            Ty::Duration(u("day")),
            Value::Duration {
                value: dec("3"),
                unit: u("day"),
            },
        ),
        (
            "half",
            Ty::Duration(u("day")),
            Value::Duration {
                value: dec("0.5"),
                unit: u("day"),
            },
        ),
        (
            "dt",
            Ty::DateTime,
            Value::DateTime(Timestamp::from_str("2027-01-20T09:00:00Z").unwrap()),
        ),
        (
            "dt2",
            Ty::DateTime,
            Value::DateTime(Timestamp::from_str("2027-01-20T10:00:00Z").unwrap()),
        ),
        ("day1", Ty::Date, Value::Date(date(2027, 2, 1))),
        ("day2", Ty::Date, Value::Date(date(2027, 2, 8))),
        ("status", Ty::Enum(id(STATUS)), status),
    ] {
        types.bind_root(sym(name), ty).unwrap();
        values.bind(sym(name), value).unwrap();
    }
    types
        .define_enum(id(STATUS), ["Draft", "Approved"].map(sym))
        .unwrap();
    (types, values)
}

fn run(src: &str) -> Result<Value, EvalError> {
    let (types, values) = env();
    let calendars = StaticCalendarProvider::new([]).unwrap();
    eval(
        &parse(src).unwrap(),
        &EvalContext {
            types: &types,
            values: &values,
            calendars: &calendars,
            predicates: &NoManagers,
        },
    )
}

fn truth(src: &str) -> bool {
    match run(src) {
        Ok(Value::Bool(b)) => b,
        other => panic!("{src}: {other:?}"),
    }
}

fn ty(src: &str) -> Result<Ty, TypeCheckError> {
    typecheck(&parse(src).unwrap(), &env().0)
}

// ---------------------------------------------------------------- equality

#[test]
fn equality_compares_each_value_kind_by_value() {
    for (src, expected) in [
        // Int and Decimal, including the mixed numeric form.
        ("i == 7", true),
        ("i == 8", false),
        ("i != 7", false),
        ("i != j", true),
        ("d == 1.25", true),
        ("d == 1.5", false),
        ("i == 7.0", true),
        // Bool, String, Date and DateTime.
        ("b == true", true),
        ("b == f", false),
        ("b != f", true),
        ("s == \"x\"", true),
        ("s == \"y\"", false),
        ("day1 == day1", true),
        ("day1 == day2", false),
        ("day1 != day2", true),
        ("dt == dt", true),
        ("dt == dt2", false),
        // Unit-bearing values of one unit compare their magnitudes.
        ("h1 == h1", true),
        ("h1 == h2", false),
        ("h1 != h2", true),
        ("h1 == quantity(1.5, hour)", true),
        ("dur == duration(3, day)", true),
        ("dur == half", false),
        // The literal-zero rule compares the magnitude with zero, either side.
        ("h1 == 0", false),
        ("0 == h1", false),
        ("h1 - h1 == 0", true),
        ("0 == dur - dur", true),
        // Enums by member.
        ("status == Approved", true),
        ("status == Draft", false),
    ] {
        assert_eq!(truth(src), expected, "{src}");
    }
}

#[test]
fn list_equality_is_elementwise_and_length_sensitive() {
    for (src, expected) in [
        ("[1, 2] == [1, 2]", true),
        ("[1, 2] == [1, 3]", false),
        ("[1, 2] == [2, 1]", false),
        ("[1] == [1, 2]", false),
        ("[1, 2] == [1]", false),
        ("[1, 2] != [1, 2]", false),
        ("[day1, day2] == [day1, day2]", true),
        ("[[1], [2]] == [[1], [2]]", true),
        ("[[1], [2]] == [[1], [3]]", false),
    ] {
        assert_eq!(truth(src), expected, "{src}");
    }
}

// ---------------------------------------------------------------- ordering and logic

#[test]
fn ordering_and_negation_follow_the_operator() {
    for (src, expected) in [
        ("i > j", true),
        ("j > i", false),
        ("i > 7", false),
        ("i >= 7", true),
        ("i < j", false),
        ("d > 1", true),
        ("h2 > h1", true),
        ("h1 > h2", false),
        // Literal zero against a unit-bearing value, either side.
        ("h1 > 0", true),
        ("0 < h1", true),
        ("0 > h1", false),
        ("0 >= dur", false),
        ("day2 > day1", true),
        ("dt < dt2", true),
        ("not b", false),
        ("not f", true),
        ("not (i > j)", false),
    ] {
        assert_eq!(truth(src), expected, "{src}");
    }
}

#[test]
fn enum_member_on_the_left_of_in_is_typed_by_the_list() {
    assert_eq!(ty("Approved in [status]"), Ok(Ty::Bool));
    assert!(truth("Approved in [status]"));
    assert!(!truth("Draft in [status]"));
}

// ---------------------------------------------------------------- arithmetic

#[test]
fn unit_bearing_arithmetic_keeps_the_kind_and_unit() {
    assert_eq!(
        run("dur + half"),
        Ok(Value::Duration {
            value: dec("3.5"),
            unit: u("day")
        })
    );
    assert_eq!(
        run("dur - half"),
        Ok(Value::Duration {
            value: dec("2.5"),
            unit: u("day")
        })
    );
    // A scalar on the left of `*` scales the right operand and keeps its kind.
    assert_eq!(
        run("2 * h1"),
        Ok(Value::Quantity {
            value: dec("3.0"),
            unit: u("hour")
        })
    );
    assert_eq!(
        run("2 * dur"),
        Ok(Value::Duration {
            value: dec("6"),
            unit: u("day")
        })
    );
    assert_eq!(
        run("h1 * 2"),
        Ok(Value::Quantity {
            value: dec("3.0"),
            unit: u("hour")
        })
    );
}

#[test]
fn date_shift_is_bounded_but_spans_the_calendar() {
    let day1 = date(2027, 2, 1);
    assert_eq!(
        run("day1 + duration(30, day)"),
        Ok(Value::Date(day1 + time::Duration::days(30)))
    );
    assert_eq!(
        run("day1 - duration(30, day)"),
        Ok(Value::Date(day1 - time::Duration::days(30)))
    );
    // Shifts of decades stay valid dates.
    assert_eq!(
        run("day1 + duration(20000, day)"),
        Ok(Value::Date(day1 + time::Duration::days(20_000)))
    );
    // Beyond year 9999, and far beyond any representable duration: a typed error, no panic.
    assert_eq!(
        run("day1 + duration(3000000, day)"),
        Err(EvalError::DateOutOfRange)
    );
    assert_eq!(
        run("day1 + duration(100000000000000000, day)"),
        Err(EvalError::DateOutOfRange)
    );
    assert_eq!(
        run("day2 - day1"),
        Ok(Value::Duration {
            value: dec("7"),
            unit: u("day")
        })
    );
}

// ---------------------------------------------------------------- typing rules

#[test]
fn typing_rejects_operands_outside_the_rules() {
    // Quantity times quantity of one unit is an invalid operand, not a unit mismatch.
    assert!(matches!(
        ty("h1 * h2"),
        Err(TypeCheckError::InvalidOperand { op, .. }) if op == "*"
    ));
    // Different units are a unit mismatch.
    assert!(matches!(
        ty("h1 + quantity(1, minute)"),
        Err(TypeCheckError::UnitMismatch { .. })
    ));
    // Dates subtract to a day Duration; they never add.
    assert_eq!(ty("day2 - day1"), Ok(Ty::Duration(u("day"))));
    assert!(matches!(
        ty("day1 + day2"),
        Err(TypeCheckError::InvalidOperand { op, .. }) if op == "+"
    ));
    // sum is numeric or unit-bearing only; min and max also order dates.
    assert!(matches!(
        ty("sum([day1, day2])"),
        Err(TypeCheckError::TypeMismatch { .. })
    ));
    assert_eq!(ty("max([day1, day2])"), Ok(Ty::Date));
    assert_eq!(ty("sum([h1, h2])"), Ok(Ty::Quantity(u("hour"))));
    // Multiplication of integers is integral.
    assert_eq!(ty("i * j"), Ok(Ty::Int));
}

#[test]
fn conditional_branches_need_a_common_type_or_a_literal_zero() {
    assert_eq!(ty("if b then h1 else 0"), Ok(Ty::Quantity(u("hour"))));
    assert_eq!(ty("if b then 0 else h1"), Ok(Ty::Quantity(u("hour"))));
    for src in ["if b then h1 else 1", "if b then 1 else h1"] {
        assert!(
            matches!(
                ty(src),
                Err(TypeCheckError::IncompatibleConditionalBranches { .. })
            ),
            "{src}"
        );
    }
}

#[test]
fn environment_rebinding_is_idempotent_only_for_the_identical_type() {
    let mut env = TypeEnv::new();
    let owner = id("entity:Request");
    env.bind_field(owner.clone(), sym("start"), Ty::Date)
        .unwrap();
    assert_eq!(
        env.bind_field(owner.clone(), sym("start"), Ty::Date),
        Ok(())
    );
    assert_eq!(
        env.bind_field(owner.clone(), sym("start"), Ty::Int),
        Err(TypeEnvError::FieldConflict {
            ref_type: owner,
            field: sym("start")
        })
    );
    env.set_calendar_ref_type(id("type:calendar")).unwrap();
    assert_eq!(env.set_calendar_ref_type(id("type:calendar")), Ok(()));
    assert_eq!(
        env.set_calendar_ref_type(id("type:other")),
        Err(TypeEnvError::CalendarTypeConflict {
            existing: id("type:calendar")
        })
    );
}

// ---------------------------------------------------------------- parser

fn on_large_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn is_nesting_limit(result: Result<Ast, ParseError>) -> bool {
    matches!(
        result,
        Err(ParseError::LimitExceeded {
            limit: "EXPR_MAX_NESTING_DEPTH",
            ..
        })
    )
}

#[test]
fn syntax_errors_carry_their_offset() {
    let error = parse("1 +").unwrap_err();
    assert!(matches!(error, ParseError::Syntax { .. }));
    assert!(error.offset().is_some());
    assert_eq!(
        parse("01").unwrap_err().offset(),
        Some(0),
        "an invalid integer at the start"
    );
    assert_eq!(ParseError::Empty.offset(), None);
}

#[test]
fn nesting_scan_counts_only_open_brackets_and_prefix_runs() {
    on_large_stack(|| {
        // Closed groups do not accumulate depth.
        let sequential = format!("{}a", "(a) + ".repeat(70));
        assert!(parse(&sequential).is_ok(), "{sequential}");
        let lists = format!("{}a", "count([a]) + ".repeat(70));
        assert!(parse(&lists).is_ok());
        // A string literal ends at its closing quote; brackets after it still count.
        let after_string = format!("\"x\" + {}a{}", "(".repeat(65), ")".repeat(65));
        assert!(is_nesting_limit(parse(&after_string)));
        // A binary minus is not part of the prefix run that follows it: 64 prefixes pass.
        let binary_then_prefix = format!("a - {}a", "-".repeat(64));
        assert!(parse(&binary_then_prefix).is_ok());
        // Operator keywords start a prefix run: 65 prefixes after `and` fail.
        let after_and = format!("b and {}a", "-".repeat(65));
        assert!(is_nesting_limit(parse(&after_and)));
        let after_if = format!("if {}a then 1 else 2", "-".repeat(65));
        assert!(is_nesting_limit(parse(&after_if)));
        // `->` is an operator, never a prefix `-`: it ends a run of 64 `not`.
        let implication = format!("{}a -> b", "not ".repeat(64));
        assert!(parse(&implication).is_ok(), "{implication}");
        let bare_arrow = format!("{}-> b", "not ".repeat(64));
        assert!(!is_nesting_limit(parse(&bare_arrow)));
    });
}
