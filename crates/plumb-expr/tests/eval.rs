//! S2.5 contract tests for PlumbExpr semantics: TypeEnv and the pilot value-type mapper,
//! typing rules, evaluation, missing values and short-circuiting, the frozen callables,
//! calendars, rounding, the example renderer, the HR expressions with a synthetic HR
//! environment, and generated well-typed expressions. The HR fixture values are compiled into
//! the tests; nothing reads fixture files. No end-to-end scenario is claimed.

use std::collections::BTreeMap;
use std::str::FromStr;

use plumb_core::{Id, Timestamp};
use plumb_expr::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use time::{Date, Month, Weekday};

// ---------------------------------------------------------------- builders

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

fn q(value: &str, unit: &str) -> Value {
    Value::Quantity {
        value: dec(value),
        unit: u(unit),
    }
}

const STATUS: &str = "attr:LeaveRequest-status";
const LEAVE_TYPE: &str = "attr:LeaveRequest-leave_type";
const PERSON: &str = "type:person";
const CALENDAR: &str = "type:calendar";
const CAL: &str = "cal:PL-Office";

/// The synthetic HR type environment (explicit; nothing is inferred).
fn hr_types() -> TypeEnv {
    let mut env = TypeEnv::new();
    let person = Ty::Ref(id(PERSON));
    let wd = Ty::Quantity(u("working_day"));
    for (name, ty) in [
        ("LeaveRequest", Ty::Ref(id("entity:LeaveRequest"))),
        ("LeaveBalance", Ty::Ref(id("entity:LeaveBalance"))),
        (
            "ApproveLeaveRequest",
            Ty::Ref(id("entity:ApproveLeaveRequest")),
        ),
        ("ApprovalRecord", Ty::Ref(id("entity:ApprovalRecord"))),
        ("actor", person.clone()),
        ("employee", person.clone()),
        ("start_date", Ty::Date),
        ("end_date", Ty::Date),
        ("day_fraction", Ty::Decimal(2)),
        ("inclusive_end", Ty::Bool),
        ("leave_type", Ty::Enum(id(LEAVE_TYPE))),
        ("RequestedWorkingDays", wd.clone()),
        ("calendar", Ty::Ref(id(CALENDAR))),
    ] {
        env.bind_root(sym(name), ty).unwrap();
    }
    for (owner, field, ty) in [
        ("entity:LeaveRequest", "start_date", Ty::Date),
        ("entity:LeaveRequest", "end_date", Ty::Date),
        ("entity:LeaveRequest", "employee", person.clone()),
        ("entity:LeaveRequest", "status", Ty::Enum(id(STATUS))),
        ("entity:LeaveBalance", "remaining_days", wd),
        ("entity:ApproveLeaveRequest", "actor", person),
    ] {
        env.bind_field(id(owner), sym(field), ty).unwrap();
    }
    env.define_enum(
        id(STATUS),
        [
            "Draft",
            "Submitted",
            "Approved",
            "Rejected",
            "Withdrawn",
            "Cancelled",
        ]
        .map(sym),
    )
    .unwrap();
    env.define_enum(id(LEAVE_TYPE), ["Annual", "Unpaid", "Sick"].map(sym))
        .unwrap();
    env.set_calendar_ref_type(id(CALENDAR)).unwrap();
    env
}

fn person(value_ref: &str) -> Value {
    Value::Ref(RefValue {
        type_ref: id(PERSON),
        value_ref: id(value_ref),
        fields: BTreeMap::new(),
    })
}

fn record(type_ref: &str, value_ref: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Ref(RefValue {
        type_ref: id(type_ref),
        value_ref: id(value_ref),
        fields: fields.into_iter().map(|(k, v)| (sym(k), v)).collect(),
    })
}

/// HR runtime values; `ApprovalRecord` is absent unless added.
fn hr_values() -> ValueEnv {
    let mut values = ValueEnv::new();
    for (name, value) in [
        (
            "LeaveRequest",
            record(
                "entity:LeaveRequest",
                "lr:1",
                vec![
                    ("start_date", Value::Date(date(2027, 2, 1))),
                    ("end_date", Value::Date(date(2027, 2, 2))),
                    ("employee", person("person:E100")),
                    (
                        "status",
                        Value::Enum {
                            enum_ref: id(STATUS),
                            member: sym("Approved"),
                        },
                    ),
                ],
            ),
        ),
        (
            "LeaveBalance",
            record(
                "entity:LeaveBalance",
                "lb:1",
                vec![("remaining_days", q("3.50", "working_day"))],
            ),
        ),
        (
            "ApproveLeaveRequest",
            record(
                "entity:ApproveLeaveRequest",
                "op:1",
                vec![("actor", person("person:M200"))],
            ),
        ),
        ("actor", person("person:M200")),
        ("employee", person("person:E100")),
        ("start_date", Value::Date(date(2027, 2, 1))),
        ("end_date", Value::Date(date(2027, 2, 1))),
        ("day_fraction", Value::Decimal(dec("0.5"))),
        ("inclusive_end", Value::Bool(true)),
        (
            "leave_type",
            Value::Enum {
                enum_ref: id(LEAVE_TYPE),
                member: sym("Annual"),
            },
        ),
        ("RequestedWorkingDays", q("2", "working_day")),
        ("calendar", record(CALENDAR, CAL, vec![])),
    ] {
        values.bind(sym(name), value).unwrap();
    }
    values
}

/// The immutable HR calendar, compiled from fixture-contract.yaml.
fn hr_calendar() -> StaticCalendarProvider {
    StaticCalendarProvider::new([CalendarDefinition::new(
        id(CAL),
        [Weekday::Saturday, Weekday::Sunday],
        [
            date(2027, 1, 1),
            date(2027, 1, 6),
            date(2027, 5, 3),
            date(2027, 11, 11),
            date(2027, 12, 25),
            date(2027, 12, 26),
        ],
    )])
    .unwrap()
}

/// M200 manages E100.
struct Managers;

impl PredicateProvider for Managers {
    fn direct_manager(
        &self,
        actor: &RefValue,
        employee: &RefValue,
    ) -> Result<bool, PredicateError> {
        Ok(actor.value_ref.as_str() == "person:M200"
            && employee.value_ref.as_str() == "person:E100")
    }
}

fn ty_of(src: &str, env: &TypeEnv) -> Result<Ty, TypeCheckError> {
    typecheck(&parse(src).unwrap(), env)
}

fn hr_ty(src: &str) -> Result<Ty, TypeCheckError> {
    ty_of(src, &hr_types())
}

fn run(src: &str, types: &TypeEnv, values: &ValueEnv) -> Result<Value, EvalError> {
    let calendars = hr_calendar();
    eval(
        &parse(src).unwrap(),
        &EvalContext {
            types,
            values,
            calendars: &calendars,
            predicates: &Managers,
        },
    )
}

fn hr_run(src: &str) -> Result<Value, EvalError> {
    run(src, &hr_types(), &hr_values())
}

/// A scalar-only environment for numeric tests.
fn scalar_env() -> (TypeEnv, ValueEnv) {
    let mut types = TypeEnv::new();
    let mut values = ValueEnv::new();
    for (name, ty, value) in [
        ("i", Ty::Int, Value::Int(7)),
        ("j", Ty::Int, Value::Int(-3)),
        ("big", Ty::Int, Value::Int(i64::MAX)),
        ("zero", Ty::Int, Value::Int(0)),
        ("d", Ty::Decimal(2), Value::Decimal(dec("1.25"))),
        ("e", Ty::Decimal(1), Value::Decimal(dec("0.5"))),
        ("b", Ty::Bool, Value::Bool(true)),
        ("f", Ty::Bool, Value::Bool(false)),
        ("s", Ty::String, Value::String("x".into())),
        ("h1", Ty::Quantity(u("hour")), q("1.5", "hour")),
        ("h2", Ty::Quantity(u("hour")), q("2", "hour")),
        ("m1", Ty::Quantity(u("minute")), q("30", "minute")),
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
        ("day1", Ty::Date, Value::Date(date(2027, 2, 1))),
        ("day2", Ty::Date, Value::Date(date(2027, 2, 8))),
        ("missing", Ty::Int, Value::Int(0)),
    ] {
        types.bind_root(sym(name), ty).unwrap();
        if name != "missing" {
            values.bind(sym(name), value).unwrap();
        }
    }
    (types, values)
}

fn s_ty(src: &str) -> Result<Ty, TypeCheckError> {
    ty_of(src, &scalar_env().0)
}

fn s_run(src: &str) -> Result<Value, EvalError> {
    let (types, values) = scalar_env();
    run(src, &types, &values)
}

// ---------------------------------------------------------------- environments and mapping

#[test]
fn eval_pilot_value_type_mapping() {
    let e = id(STATUS);
    for (value_type, ty) in [
        ("String", Ty::String),
        ("Int", Ty::Int),
        ("Bool", Ty::Bool),
        ("Date", Ty::Date),
        ("DateTime", Ty::DateTime),
        ("Decimal(0)", Ty::Decimal(0)),
        ("Decimal(2)", Ty::Decimal(2)),
        ("Decimal(28)", Ty::Decimal(28)),
    ] {
        assert_eq!(
            map_pilot_value_type(value_type, None, None),
            Ok(ty),
            "{value_type}"
        );
    }
    assert_eq!(
        map_pilot_value_type("Enum", None, Some(&e)),
        Ok(Ty::Enum(e.clone()))
    );
    assert_eq!(
        map_pilot_value_type("Enum", None, None),
        Err(TypeBindingError::MissingEnumIdentity)
    );
    assert_eq!(
        map_pilot_value_type("Decimal(2)", Some("working_day"), None),
        Ok(Ty::Quantity(u("working_day")))
    );
    assert_eq!(
        map_pilot_value_type("Int", Some("hour"), None),
        Ok(Ty::Quantity(u("hour")))
    );
    for value_type in ["String", "Bool", "Date", "DateTime"] {
        assert_eq!(
            map_pilot_value_type(value_type, Some("day"), None),
            Err(TypeBindingError::UnitOnNonNumericType(value_type.into()))
        );
    }
    assert_eq!(
        map_pilot_value_type("Enum", Some("day"), Some(&e)),
        Err(TypeBindingError::UnitOnNonNumericType("Enum".into()))
    );
    for unsupported in [
        "Text",
        "Money",
        "UUID",
        "Object",
        "string",
        "Decimal(29)",
        "Decimal(02)",
        "Decimal()",
        "Decimal(2",
        "Decimal",
    ] {
        assert_eq!(
            map_pilot_value_type(unsupported, None, None),
            Err(TypeBindingError::UnsupportedValueType(unsupported.into())),
            "{unsupported}"
        );
    }
    assert!(matches!(
        map_pilot_value_type("Int", Some("Day"), None),
        Err(TypeBindingError::InvalidUnit(_))
    ));
}

#[test]
fn eval_type_env_bindings() {
    let mut env = TypeEnv::new();
    env.bind_root(sym("x"), Ty::Int).unwrap();
    env.bind_root(sym("x"), Ty::Int).unwrap();
    assert_eq!(
        env.bind_root(sym("x"), Ty::Bool),
        Err(TypeEnvError::BindingConflict { name: sym("x") })
    );
    assert_eq!(env.root(&sym("x")), Some(&Ty::Int));
    assert!(matches!(
        env.bind_root(sym("y"), Ty::Decimal(29)),
        Err(TypeEnvError::InvalidType(_))
    ));
    env.bind_field(id("type:a"), sym("f"), Ty::Date).unwrap();
    assert!(matches!(
        env.bind_field(id("type:a"), sym("f"), Ty::Int),
        Err(TypeEnvError::FieldConflict { .. })
    ));
    env.define_enum(id("enum:e"), [sym("A"), sym("B")]).unwrap();
    env.define_enum(id("enum:e"), [sym("B"), sym("A")]).unwrap();
    assert!(matches!(
        env.define_enum(id("enum:e"), [sym("A")]),
        Err(TypeEnvError::EnumConflict { .. })
    ));
    env.set_calendar_ref_type(id(CALENDAR)).unwrap();
    assert!(matches!(
        env.set_calendar_ref_type(id("type:other")),
        Err(TypeEnvError::CalendarTypeConflict { .. })
    ));
    let mut values = ValueEnv::new();
    values.bind(sym("x"), Value::Int(1)).unwrap();
    values.bind(sym("x"), Value::Int(1)).unwrap();
    assert!(matches!(
        values.bind(sym("x"), Value::Int(2)),
        Err(ValueEnvError::BindingConflict { .. })
    ));
}

// ---------------------------------------------------------------- typing

#[test]
fn eval_literal_and_numeric_typing() {
    for (src, ty) in [
        ("1", Ty::Int),
        ("1.50", Ty::Decimal(2)),
        ("true", Ty::Bool),
        ("\"s\"", Ty::String),
        ("date(\"2027-02-01\")", Ty::Date),
        ("datetime(\"2027-02-01T00:00:00Z\")", Ty::DateTime),
        ("duration(5, day)", Ty::Duration(u("day"))),
        ("quantity(0.5, working_day)", Ty::Quantity(u("working_day"))),
        ("i + j", Ty::Int),
        ("i + d", Ty::Decimal(2)),
        ("d - e", Ty::Decimal(2)),
        ("i * j", Ty::Int),
        ("i * d", Ty::Decimal(2)),
        ("d * e", Ty::Decimal(3)),
        ("i / j", Ty::Decimal(28)),
        ("d / e", Ty::Decimal(28)),
        ("i % j", Ty::Int),
        ("d % i", Ty::Decimal(2)),
        ("d % e", Ty::Decimal(2)),
        ("-i", Ty::Int),
        ("-d", Ty::Decimal(2)),
        ("[1, 2.5]", Ty::List(Box::new(Ty::Decimal(1)))),
        ("if b then 1 else 2.50", Ty::Decimal(2)),
    ] {
        assert_eq!(s_ty(src), Ok(ty), "{src}");
    }
    let many = format!("1.{} * 1.{}", "1".repeat(20), "1".repeat(20));
    assert_eq!(s_ty(&many), Ok(Ty::Decimal(28)));
    for bad in [
        "i + b",
        "s + s",
        "not i",
        "-b",
        "-s",
        "b and i",
        "i < s",
        "b < f",
        "s < s",
        "\"a\" == 1",
    ] {
        assert!(s_ty(bad).is_err(), "{bad}");
    }
    assert!(matches!(
        s_ty("i + b"),
        Err(TypeCheckError::InvalidOperand { .. })
    ));
    assert!(matches!(
        s_ty("not i"),
        Err(TypeCheckError::TypeMismatch { .. })
    ));
}

#[test]
fn eval_unit_and_date_typing() {
    let hour = Ty::Quantity(u("hour"));
    let day = Ty::Duration(u("day"));
    for (src, ty) in [
        ("h1 + h2", hour.clone()),
        ("h1 - h2", hour.clone()),
        ("h1 * 2", hour.clone()),
        ("2 * h1", hour.clone()),
        ("h1 * d", hour.clone()),
        ("h1 / 2", hour.clone()),
        ("-h1", hour.clone()),
        ("dur + dur", day.clone()),
        ("dur * 2", day.clone()),
        ("day1 + dur", Ty::Date),
        ("day1 - dur", Ty::Date),
        ("day1 - day2", day.clone()),
        ("day1 + duration(1, day)", Ty::Date),
        ("h1 < h2", Ty::Bool),
        ("dt < dt", Ty::Bool),
        ("dt == dt", Ty::Bool),
        ("day1 <= day2", Ty::Bool),
        ("h1 >= 0", Ty::Bool),
        ("0.00 == h1", Ty::Bool),
        ("dur > 0", Ty::Bool),
        ("if b then h1 else 0", hour.clone()),
        ("if b then 0.0 else h1", hour.clone()),
    ] {
        assert_eq!(s_ty(src), Ok(ty), "{src}");
    }
    assert!(matches!(
        s_ty("h1 + m1"),
        Err(TypeCheckError::UnitMismatch { .. })
    ));
    assert!(matches!(
        s_ty("h1 == m1"),
        Err(TypeCheckError::UnitMismatch { .. })
    ));
    for forbidden in [
        "h1 / h2",
        "h1 * h2",
        "dur / dur",
        "dur * dur",
        "h1 + 1",
        "dur + 1",
        "2 / h1",
        "2 / dur",
        "h1 % 2",
        "h1 + 0",
        "h1 >= 1",
        "h1 == 0.5",
        "h1 > -0",
        "day1 + duration(1, hour)",
        "day1 + quantity(1, day)",
        "day1 + 1",
        "dur + day1",
        "dt + dur",
        "dt - dt",
        "if b then h1 else 1",
        "[h1, 0]",
        "h1 in [0]",
    ] {
        assert!(s_ty(forbidden).is_err(), "{forbidden}");
    }
}

#[test]
fn eval_enum_ref_list_and_conditional_typing() {
    // Contextual enum members.
    for src in [
        "LeaveRequest.status == Approved",
        "Approved == LeaveRequest.status",
        "LeaveRequest.status != Draft",
        "leave_type in [Annual, Sick]",
        "leave_type in []",
        "if leave_type == Annual then leave_type else Sick",
    ] {
        assert!(hr_ty(src).is_ok(), "{src}");
    }
    assert_eq!(
        hr_ty("[Annual, leave_type]"),
        Ok(Ty::List(Box::new(Ty::Enum(id(LEAVE_TYPE)))))
    );
    // No enum without context, no choice by uniqueness, members are per-enum.
    assert!(matches!(
        hr_ty("Approved"),
        Err(TypeCheckError::UnknownIdentifier { .. })
    ));
    assert!(matches!(
        hr_ty("[Annual, Sick]"),
        Err(TypeCheckError::UnknownIdentifier { .. })
    ));
    assert!(matches!(
        hr_ty("leave_type == Approved"),
        Err(TypeCheckError::UnknownEnumMember { .. })
    ));
    assert!(matches!(
        hr_ty("LeaveRequest.status == leave_type"),
        Err(TypeCheckError::InvalidOperand { .. })
    ));
    assert!(matches!(
        hr_ty("leave_type < leave_type"),
        Err(TypeCheckError::InvalidOperand { .. })
    ));
    // Ref navigation.
    assert_eq!(hr_ty("LeaveRequest.start_date"), Ok(Ty::Date));
    assert_eq!(hr_ty("LeaveRequest.employee"), Ok(Ty::Ref(id(PERSON))));
    assert_eq!(hr_ty("LeaveRequest.employee == employee"), Ok(Ty::Bool));
    assert!(matches!(
        hr_ty("LeaveRequest.nope"),
        Err(TypeCheckError::UnknownRefField { .. })
    ));
    assert!(matches!(
        hr_ty("start_date.year"),
        Err(TypeCheckError::CannotNavigateNonRef { .. })
    ));
    assert!(matches!(
        hr_ty("LeaveRequest.start_date.day"),
        Err(TypeCheckError::CannotNavigateNonRef { .. })
    ));
    assert!(matches!(
        hr_ty("Nobody.field"),
        Err(TypeCheckError::UnknownIdentifier { .. })
    ));
    assert!(matches!(
        hr_ty("LeaveRequest < LeaveRequest"),
        Err(TypeCheckError::InvalidOperand { .. })
    ));
    assert!(matches!(
        hr_ty("LeaveRequest == calendar"),
        Err(TypeCheckError::InvalidOperand { .. })
    ));
    // Lists, in and conditionals.
    assert!(matches!(
        s_ty("[]"),
        Err(TypeCheckError::EmptyListTypeUnknown)
    ));
    assert!(matches!(
        s_ty("[1, true]"),
        Err(TypeCheckError::IncompatibleListElements { .. })
    ));
    assert_eq!(s_ty("i in [1, 2.5]"), Ok(Ty::Bool));
    assert_eq!(s_ty("[1] == [2.5]"), Ok(Ty::Bool));
    assert!(matches!(
        s_ty("i in 5"),
        Err(TypeCheckError::TypeMismatch { .. })
    ));
    assert!(s_ty("i in [true]").is_err());
    assert!(matches!(
        s_ty("if i then 1 else 2"),
        Err(TypeCheckError::TypeMismatch { .. })
    ));
    assert!(matches!(
        s_ty("if b then 1 else true"),
        Err(TypeCheckError::IncompatibleConditionalBranches { .. })
    ));
    assert_eq!(s_ty("b -> f"), Ok(Ty::Bool));
    assert!(matches!(
        s_ty("b -> i"),
        Err(TypeCheckError::TypeMismatch { .. })
    ));
}

#[test]
fn eval_function_typing() {
    let env = hr_types();
    for (src, ty) in [
        ("exists(ApprovalRecord)", Ty::Bool),
        ("exists(LeaveRequest.employee)", Ty::Bool),
        ("direct_manager(actor, employee)", Ty::Bool),
        (
            "working_days([start_date, end_date], calendar, true)",
            Ty::Quantity(u("working_day")),
        ),
        ("days_between(start_date, end_date)", Ty::Int),
        ("count([])", Ty::Int),
        ("count([start_date])", Ty::Int),
        ("any([])", Ty::Bool),
        ("all([true, inclusive_end])", Ty::Bool),
        ("sum([1, 2.5])", Ty::Decimal(1)),
        (
            "sum([RequestedWorkingDays, LeaveBalance.remaining_days])",
            Ty::Quantity(u("working_day")),
        ),
        ("min([start_date, end_date])", Ty::Date),
        ("max([1, 2])", Ty::Int),
    ] {
        assert_eq!(ty_of(src, &env), Ok(ty), "{src}");
    }
    let err = |src: &str| ty_of(src, &env).unwrap_err();
    assert!(matches!(err("foo(1)"), TypeCheckError::UnknownFunction { name } if name == "foo"));
    assert!(matches!(
        err("system(rm)"),
        TypeCheckError::UnknownFunction { .. }
    ));
    assert!(matches!(
        err("no_overlap(Approved, LeaveRequest, employee)"),
        TypeCheckError::UnknownFunction { .. }
    ));
    assert!(
        matches!(err("as_of(start_date)"), TypeCheckError::DeferredFunction { name } if name == "as_of")
    );
    assert!(matches!(
        err("exists(1)"),
        TypeCheckError::InvalidOperand { .. }
    ));
    assert!(matches!(
        err("exists(Nobody)"),
        TypeCheckError::UnknownIdentifier { .. }
    ));
    assert!(matches!(
        err("exists(a, b)"),
        TypeCheckError::WrongArity {
            expected: 1,
            found: 2,
            ..
        }
    ));
    assert!(matches!(
        err("direct_manager(actor, start_date)"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("working_days([start_date], calendar, true)"),
        TypeCheckError::InvalidPeriodType
    ));
    assert!(matches!(
        err("working_days([start_date, end_date, end_date], calendar, true)"),
        TypeCheckError::InvalidPeriodType
    ));
    assert!(matches!(
        err("working_days([start_date, end_date], actor, true)"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("working_days([start_date, end_date], calendar, 1)"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("days_between(start_date, 1)"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("sum([])"),
        TypeCheckError::EmptyListTypeUnknown
    ));
    assert!(matches!(
        err("sum([true])"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("min([LeaveRequest.status])"),
        TypeCheckError::TypeMismatch { .. }
    ));
    assert!(matches!(
        err("any([1])"),
        TypeCheckError::TypeMismatch { .. }
    ));
    let mut no_calendar = TypeEnv::new();
    no_calendar.bind_root(sym("a"), Ty::Date).unwrap();
    no_calendar
        .bind_root(sym("c"), Ty::Ref(id(CALENDAR)))
        .unwrap();
    assert!(matches!(
        ty_of("working_days([a, a], c, true)", &no_calendar),
        Err(TypeCheckError::MissingCalendarType)
    ));
    // Units never mix in aggregates.
    let (scalar_types, _) = scalar_env();
    assert!(ty_of("sum([h1, m1])", &scalar_types).is_err());
    assert!(ty_of("max([h1, m1])", &scalar_types).is_err());
}

// ---------------------------------------------------------------- evaluation

#[test]
fn eval_numeric_and_unit_evaluation() {
    for (src, value) in [
        ("i + j", Value::Int(4)),
        ("i * j", Value::Int(-21)),
        ("i - j", Value::Int(10)),
        ("i / 2", Value::Decimal(dec("3.5"))),
        ("i % j", Value::Int(1)),
        ("j % i", Value::Int(-3)),
        ("-7.5 % 2", Value::Decimal(dec("-1.5"))),
        ("d + i", Value::Decimal(dec("8.25"))),
        ("d * e", Value::Decimal(dec("0.625"))),
        (
            "1 / 3",
            Value::Decimal(dec("0.3333333333333333333333333333")),
        ),
        ("h1 + h2", q("3.5", "hour")),
        ("h1 * 2", q("3.0", "hour")),
        ("h2 / 4", q("0.5", "hour")),
        (
            "dur * 2",
            Value::Duration {
                value: dec("6"),
                unit: u("day"),
            },
        ),
        ("day1 + dur", Value::Date(date(2027, 2, 4))),
        ("day2 - dur", Value::Date(date(2027, 2, 5))),
        (
            "day1 - day2",
            Value::Duration {
                value: dec("-7"),
                unit: u("day"),
            },
        ),
        (
            "day2 - day1",
            Value::Duration {
                value: dec("7"),
                unit: u("day"),
            },
        ),
        ("h1 >= 0", Value::Bool(true)),
        ("-h1 < 0", Value::Bool(true)),
        ("dt <= dt", Value::Bool(true)),
        ("i == 7.0", Value::Bool(true)),
        (
            "[1, 2.5]",
            Value::List(vec![Value::Decimal(dec("1")), Value::Decimal(dec("2.5"))]),
        ),
        ("if b then h1 else 0", q("1.5", "hour")),
        ("if f then h1 else 0.00", q("0.00", "hour")),
        ("if f then 1 else 2.50", Value::Decimal(dec("2.50"))),
    ] {
        assert_eq!(s_run(src), Ok(value), "{src}");
    }
    assert_eq!(s_run("big + 1"), Err(EvalError::Overflow));
    assert_eq!(s_run("big * 2"), Err(EvalError::Overflow));
    assert_eq!(s_run("-big - 2"), Err(EvalError::Overflow));
    assert_eq!(s_run("i / zero"), Err(EvalError::DivisionByZero));
    assert_eq!(s_run("i % zero"), Err(EvalError::DivisionByZero));
    assert_eq!(s_run("d / 0.0"), Err(EvalError::DivisionByZero));
    assert_eq!(s_run("h1 / 0"), Err(EvalError::DivisionByZero));
    assert_eq!(
        s_run("day1 + half"),
        Err(EvalError::NonIntegralDateDuration)
    );
    assert_eq!(
        s_run("date(\"9999-12-31\") + dur"),
        Err(EvalError::DateOutOfRange)
    );
}

#[test]
fn eval_missing_values_and_short_circuit() {
    assert_eq!(
        s_run("missing"),
        Err(EvalError::MissingValue {
            identifier: "missing".into()
        })
    );
    assert_eq!(
        s_run("missing + 1"),
        Err(EvalError::MissingValue {
            identifier: "missing".into()
        })
    );
    assert_eq!(s_run("f and missing == 1"), Ok(Value::Bool(false)));
    assert_eq!(s_run("b or missing == 1"), Ok(Value::Bool(true)));
    assert_eq!(s_run("f -> missing == 1"), Ok(Value::Bool(true)));
    assert_eq!(s_run("if b then 1 else missing"), Ok(Value::Int(1)));
    assert_eq!(s_run("if f then missing else 2"), Ok(Value::Int(2)));
    assert_eq!(
        s_run("b and missing == 1"),
        Err(EvalError::MissingValue {
            identifier: "missing".into()
        })
    );
    assert_eq!(s_run("exists(missing)"), Ok(Value::Bool(false)));
    assert_eq!(s_run("exists(i)"), Ok(Value::Bool(true)));
    // Typecheck still checks unselected branches.
    assert!(matches!(
        s_run("if b then 1 else true"),
        Err(EvalError::Type(_))
    ));
    assert!(matches!(
        s_run("f and (1 + true == 2)"),
        Err(EvalError::Type(_))
    ));
}

#[test]
fn eval_value_type_and_ref_checks() {
    let types = hr_types();
    // A consumed value must conform to its type.
    let mut values = hr_values();
    let mut wrong = ValueEnv::new();
    wrong
        .bind(sym("day_fraction"), Value::Decimal(dec("0.125")))
        .unwrap();
    assert!(matches!(
        run("day_fraction", &types, &wrong),
        Err(EvalError::ValueTypeMismatch { .. })
    ));
    let mut wrong = ValueEnv::new();
    wrong.bind(sym("inclusive_end"), Value::Int(1)).unwrap();
    assert!(matches!(
        run("inclusive_end", &types, &wrong),
        Err(EvalError::ValueTypeMismatch { .. })
    ));
    let mut wrong = ValueEnv::new();
    wrong
        .bind(sym("RequestedWorkingDays"), q("2", "day"))
        .unwrap();
    assert!(matches!(
        run("RequestedWorkingDays", &types, &wrong),
        Err(EvalError::ValueTypeMismatch { .. })
    ));
    let mut wrong = ValueEnv::new();
    wrong
        .bind(
            sym("leave_type"),
            Value::Enum {
                enum_ref: id(LEAVE_TYPE),
                member: sym("Holiday"),
            },
        )
        .unwrap();
    assert!(matches!(
        run("leave_type", &types, &wrong),
        Err(EvalError::ValueTypeMismatch { .. })
    ));
    let mut wrong = ValueEnv::new();
    wrong
        .bind(
            sym("LeaveRequest"),
            record(
                "entity:Other",
                "lr:9",
                vec![("start_date", Value::Date(date(2027, 1, 1)))],
            ),
        )
        .unwrap();
    assert!(matches!(
        run("LeaveRequest.start_date", &types, &wrong),
        Err(EvalError::ValueTypeMismatch { .. })
    ));
    // Ref navigation and identity.
    assert_eq!(
        run("LeaveRequest.start_date", &types, &values),
        Ok(Value::Date(date(2027, 2, 1)))
    );
    assert_eq!(
        run("LeaveRequest.employee == employee", &types, &values),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run(
            "ApproveLeaveRequest.actor != LeaveRequest.employee",
            &types,
            &values
        ),
        Ok(Value::Bool(true))
    );
    // Equality uses the reference identity, not embedded fields.
    values
        .bind(
            sym("ApprovalRecord"),
            record("entity:ApprovalRecord", "ar:1", vec![]),
        )
        .unwrap();
    let mut same_ref = ValueEnv::new();
    same_ref
        .bind(
            sym("actor"),
            record(PERSON, "person:E100", vec![("x", Value::Int(1))]),
        )
        .unwrap();
    same_ref
        .bind(sym("employee"), person("person:E100"))
        .unwrap();
    assert_eq!(
        run("actor == employee", &types, &same_ref),
        Ok(Value::Bool(true))
    );
    let mut partial = ValueEnv::new();
    partial
        .bind(
            sym("LeaveRequest"),
            record("entity:LeaveRequest", "lr:1", vec![]),
        )
        .unwrap();
    assert_eq!(
        run("LeaveRequest.start_date", &types, &partial),
        Err(EvalError::MissingValue {
            identifier: "LeaveRequest.start_date".into()
        })
    );
    assert_eq!(
        run("exists(LeaveRequest.start_date)", &types, &partial),
        Ok(Value::Bool(false))
    );
    // Contextual enum values need no ValueEnv binding.
    assert!(matches!(
        run("[Annual, Sick]", &types, &ValueEnv::new()),
        Err(EvalError::Type(TypeCheckError::UnknownIdentifier { .. }))
    ));
    assert_eq!(
        run("leave_type in [Annual, Sick]", &types, &hr_values()),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run("leave_type in []", &types, &hr_values()),
        Ok(Value::Bool(false))
    );
    assert_eq!(
        run("LeaveRequest.status == Approved", &types, &hr_values()),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run("Draft == LeaveRequest.status", &types, &hr_values()),
        Ok(Value::Bool(false))
    );
}

#[test]
fn eval_hr_expressions() {
    // Typing of the six representative S2.4 expressions with the explicit HR environment.
    let wd = Ty::Quantity(u("working_day"));
    for (src, ty) in [
        ("LeaveRequest.start_date <= LeaveRequest.end_date", Ty::Bool),
        ("LeaveBalance.remaining_days >= 0", Ty::Bool),
        ("LeaveRequest.status == Approved -> exists(ApprovalRecord)", Ty::Bool),
        ("ApproveLeaveRequest.actor != LeaveRequest.employee and direct_manager(actor, employee)", Ty::Bool),
        ("working_days([start_date, end_date], calendar, inclusive_end) * day_fraction", wd.clone()),
        ("if leave_type == Annual then RequestedWorkingDays else 0", wd.clone()),
    ] {
        assert_eq!(hr_ty(src), Ok(ty), "{src}");
    }
    // The literal-zero rule is narrow.
    assert!(hr_ty("LeaveBalance.remaining_days >= 1").is_err());
    assert!(matches!(
        hr_ty("if leave_type == Annual then RequestedWorkingDays else 1"),
        Err(TypeCheckError::IncompatibleConditionalBranches { .. })
    ));
    // Evaluation.
    assert_eq!(
        hr_run("LeaveRequest.start_date <= LeaveRequest.end_date"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        hr_run("LeaveBalance.remaining_days >= 0"),
        Ok(Value::Bool(true))
    );
    // Approved status: the implication needs an ApprovalRecord.
    assert_eq!(
        hr_run("LeaveRequest.status == Approved -> exists(ApprovalRecord)"),
        Ok(Value::Bool(false))
    );
    let mut with_record = hr_values();
    with_record
        .bind(
            sym("ApprovalRecord"),
            record("entity:ApprovalRecord", "ar:1", vec![]),
        )
        .unwrap();
    assert_eq!(
        run(
            "LeaveRequest.status == Approved -> exists(ApprovalRecord)",
            &hr_types(),
            &with_record
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        hr_run("ApproveLeaveRequest.actor != LeaveRequest.employee and direct_manager(actor, employee)"),
        Ok(Value::Bool(true))
    );
    // The accepted half-day answer: 1 working day * 0.5.
    assert_eq!(
        hr_run("working_days([start_date, end_date], calendar, inclusive_end) * day_fraction"),
        Ok(q("0.5", "working_day"))
    );
    assert_eq!(
        hr_run("if leave_type == Annual then RequestedWorkingDays else 0"),
        Ok(q("2", "working_day"))
    );
    let mut sick = ValueEnv::new();
    sick.bind(
        sym("leave_type"),
        Value::Enum {
            enum_ref: id(LEAVE_TYPE),
            member: sym("Sick"),
        },
    )
    .unwrap();
    assert_eq!(
        run(
            "if leave_type == Annual then RequestedWorkingDays else 0",
            &hr_types(),
            &sick
        ),
        Ok(q("0", "working_day")),
        "a zero quantity, not an Int, and the unselected branch needs no value"
    );
    // direct_manager uses only the injected provider.
    let mut other = ValueEnv::new();
    other.bind(sym("actor"), person("person:E300")).unwrap();
    other.bind(sym("employee"), person("person:E100")).unwrap();
    assert_eq!(
        run("direct_manager(actor, employee)", &hr_types(), &other),
        Ok(Value::Bool(false))
    );
}

#[test]
fn eval_working_days_and_days_between_tables() {
    let types = hr_types();
    let wd = |start: Date, end: Date, inclusive: bool| {
        let mut values = ValueEnv::new();
        values.bind(sym("start_date"), Value::Date(start)).unwrap();
        values.bind(sym("end_date"), Value::Date(end)).unwrap();
        values
            .bind(sym("inclusive_end"), Value::Bool(inclusive))
            .unwrap();
        values
            .bind(sym("calendar"), record(CALENDAR, CAL, vec![]))
            .unwrap();
        run(
            "working_days([start_date, end_date], calendar, inclusive_end)",
            &types,
            &values,
        )
    };
    let db = |start: Date, end: Date| {
        let mut values = ValueEnv::new();
        values.bind(sym("start_date"), Value::Date(start)).unwrap();
        values.bind(sym("end_date"), Value::Date(end)).unwrap();
        run("days_between(start_date, end_date)", &types, &values)
    };
    for (start, end, inclusive, expected) in [
        (date(2027, 2, 1), date(2027, 2, 1), true, "1"),
        (date(2027, 2, 1), date(2027, 2, 1), false, "0"),
        (date(2027, 2, 1), date(2027, 2, 2), true, "2"),
        (date(2027, 2, 5), date(2027, 2, 8), true, "2"),
        (date(2027, 1, 5), date(2027, 1, 7), true, "2"),
        (date(2027, 1, 6), date(2027, 1, 6), true, "0"),
    ] {
        assert_eq!(
            wd(start, end, inclusive),
            Ok(q(expected, "working_day")),
            "{start}..{end} {inclusive}"
        );
    }
    for (start, end, expected) in [
        (date(2027, 2, 1), date(2027, 2, 1), 0),
        (date(2027, 2, 1), date(2027, 2, 2), 1),
        (date(2027, 2, 5), date(2027, 2, 8), 3),
        (date(2027, 1, 5), date(2027, 1, 7), 2),
    ] {
        assert_eq!(db(start, end), Ok(Value::Int(expected)));
    }
    assert_eq!(
        wd(date(2027, 2, 2), date(2027, 2, 1), true),
        Err(EvalError::InvalidPeriod)
    );
    assert_eq!(
        db(date(2027, 2, 2), date(2027, 2, 1)),
        Err(EvalError::InvalidPeriod)
    );
    // Unknown calendar: no fallback.
    let hr = hr_values();
    let mut values = ValueEnv::new();
    for name in ["start_date", "end_date", "inclusive_end"] {
        values
            .bind(sym(name), hr.get(&sym(name)).unwrap().clone())
            .unwrap();
    }
    values
        .bind(sym("calendar"), record(CALENDAR, "cal:Nowhere", vec![]))
        .unwrap();
    assert!(matches!(
        run(
            "working_days([start_date, end_date], calendar, inclusive_end)",
            &types,
            &values
        ),
        Err(EvalError::Calendar(CalendarError::UnknownCalendar { .. }))
    ));
    // A runtime period list of the wrong length.
    let mut period_types = hr_types();
    period_types
        .bind_root(sym("period"), Ty::List(Box::new(Ty::Date)))
        .unwrap();
    let mut period_values = hr_values();
    period_values
        .bind(
            sym("period"),
            Value::List(vec![Value::Date(date(2027, 2, 1))]),
        )
        .unwrap();
    assert_eq!(
        run(
            "working_days(period, calendar, true)",
            &period_types,
            &period_values
        ),
        Err(EvalError::InvalidPeriodLength { found: 1 })
    );
    assert!(matches!(
        StaticCalendarProvider::new([
            CalendarDefinition::new(id(CAL), [], []),
            CalendarDefinition::new(id(CAL), [], []),
        ]),
        Err(CalendarError::DuplicateCalendar { .. })
    ));
}

#[test]
fn eval_aggregates() {
    for (src, value) in [
        ("sum([1, 2, 3])", Value::Int(6)),
        ("sum([1.0, 2.50])", Value::Decimal(dec("3.50"))),
        ("sum([1, 2.50])", Value::Decimal(dec("3.50"))),
        ("count([])", Value::Int(0)),
        ("count([1, 2])", Value::Int(2)),
        ("any([])", Value::Bool(false)),
        ("all([])", Value::Bool(true)),
        ("any([false, true])", Value::Bool(true)),
        ("all([true, true])", Value::Bool(true)),
        ("all([true, false])", Value::Bool(false)),
        ("min([3, 1, 2])", Value::Int(1)),
        ("max([3, 1, 2])", Value::Int(3)),
        ("max([1, 2.5])", Value::Decimal(dec("2.5"))),
        ("min([1, 2.5])", Value::Decimal(dec("1"))),
        ("sum([h1, h2])", q("3.5", "hour")),
        ("min([h1, h2])", q("1.5", "hour")),
        ("max([h1, h2])", q("2", "hour")),
        ("min([day1, day2])", Value::Date(date(2027, 2, 1))),
    ] {
        assert_eq!(s_run(src), Ok(value), "{src}");
    }
    let (mut types, mut values) = scalar_env();
    types
        .bind_root(sym("none"), Ty::List(Box::new(Ty::Int)))
        .unwrap();
    values.bind(sym("none"), Value::List(vec![])).unwrap();
    for function in ["sum", "min", "max"] {
        assert_eq!(
            run(&format!("{function}(none)"), &types, &values),
            Err(EvalError::EmptyAggregate {
                function: function.into()
            })
        );
    }
    assert!(matches!(s_run("sum([h1, m1])"), Err(EvalError::Type(_))));
}

// ---------------------------------------------------------------- rounding

#[test]
fn eval_rounding_table() {
    let r = |v: &str, spec: RoundingSpec| round_decimal(dec(v), &spec).unwrap().to_string();
    let half_up = RoundingSpec::HalfUp { scale: 2 };
    let half_even = RoundingSpec::HalfEven { scale: 2 };
    let floor = RoundingSpec::Floor { scale: 2 };
    let ceil = RoundingSpec::Ceil { scale: 2 };
    let step = RoundingSpec::ToStep { step: dec("0.5") };
    for (value, spec, expected) in [
        ("1.234", half_up, "1.23"),
        ("1.235", half_up, "1.24"),
        ("-1.234", half_up, "-1.23"),
        ("-1.235", half_up, "-1.24"),
        ("1.245", half_even, "1.24"),
        ("1.255", half_even, "1.26"),
        ("-1.245", half_even, "-1.24"),
        ("-1.255", half_even, "-1.26"),
        ("1.239", floor, "1.23"),
        ("-1.231", floor, "-1.24"),
        ("1.231", ceil, "1.24"),
        ("-1.239", ceil, "-1.23"),
        ("1.24", step, "1.0"),
        ("1.25", step, "1.5"),
        ("1.26", step, "1.5"),
        ("-1.24", step, "-1.0"),
        ("-1.25", step, "-1.5"),
        ("-1.26", step, "-1.5"),
        ("1.2", half_up, "1.20"),
        ("7", RoundingSpec::ToStep { step: dec("5") }, "5"),
        ("7.5", RoundingSpec::ToStep { step: dec("5") }, "10"),
        ("0.1234", RoundingSpec::ToStep { step: dec("0.25") }, "0.00"),
    ] {
        assert_eq!(r(value, spec), expected, "{value} {spec}");
    }
    // Hotfix 039: scale-based results carry exactly the requested scale.
    let padded = round_decimal(dec("1.2"), &RoundingSpec::HalfUp { scale: 2 }).unwrap();
    assert_eq!(padded.to_string(), "1.20");
    assert_eq!(padded.scale(), 2);
    assert_eq!(
        round_decimal(dec("1"), &RoundingSpec::HalfUp { scale: 29 }),
        Err(RoundingError::InvalidScale(29))
    );
    assert_eq!(
        round_decimal(dec("1"), &RoundingSpec::ToStep { step: dec("0") }),
        Err(RoundingError::InvalidStep)
    );
    assert_eq!(
        round_decimal(dec("1"), &RoundingSpec::ToStep { step: dec("-0.5") }),
        Err(RoundingError::InvalidStep)
    );
}

#[test]
fn eval_rounding_spec_strings() {
    for (text, spec) in [
        ("half_up(2)", RoundingSpec::HalfUp { scale: 2 }),
        ("half_even(2)", RoundingSpec::HalfEven { scale: 2 }),
        ("floor(2)", RoundingSpec::Floor { scale: 2 }),
        ("floor(0)", RoundingSpec::Floor { scale: 0 }),
        ("ceil(2)", RoundingSpec::Ceil { scale: 2 }),
        ("half_up(28)", RoundingSpec::HalfUp { scale: 28 }),
        ("to_step(0.5)", RoundingSpec::ToStep { step: dec("0.5") }),
        ("to_step(0.50)", RoundingSpec::ToStep { step: dec("0.50") }),
        ("to_step(5)", RoundingSpec::ToStep { step: dec("5") }),
    ] {
        let parsed = RoundingSpec::from_str(text).unwrap();
        assert_eq!(parsed, spec, "{text}");
        assert_eq!(parsed.to_string(), text);
        assert_eq!(RoundingSpec::from_str(&spec.to_string()), Ok(spec));
    }
    for bad in [
        "half-up(2)",
        "HALF_UP(2)",
        "half_up()",
        "half_up(29)",
        "to_step(0)",
        "to_step(-0.5)",
        "to_step(1e-2)",
        "unknown(2)",
        "half_up(2) ",
        " half_up(2)",
        "half_up( 2)",
        "half_up(02)",
        "half_up(2",
        "half_up",
        "to_step(.5)",
        "to_step(0.)",
        "to_step(00.5)",
        "",
    ] {
        assert!(RoundingSpec::from_str(bad).is_err(), "{bad}");
    }
    assert_eq!(
        RoundingSpec::from_str("half_up(29)"),
        Err(RoundingError::InvalidScale(29))
    );
    assert_eq!(
        RoundingSpec::from_str("to_step(0)"),
        Err(RoundingError::InvalidStep)
    );
    // Hotfix 039: an integer-valued step is a positive decimal; exponents are not.
    let five = RoundingSpec::from_str("to_step(5)").unwrap();
    assert_eq!(five, RoundingSpec::ToStep { step: dec("5") });
    assert_eq!(five.to_string(), "to_step(5)");
    for bad in ["to_step(0)", "to_step(-0.5)", "to_step(1e-2)"] {
        assert!(RoundingSpec::from_str(bad).is_err(), "{bad}");
    }
}

// ---------------------------------------------------------------- renderer

#[test]
fn eval_renderer_snapshots() {
    let types = hr_types();
    let render = |src: &str, values: &ValueEnv| {
        let ast = parse(src).unwrap();
        let result = run(src, &types, values).unwrap();
        render_example(&ast, &types, values, &result)
    };
    let mut period = ValueEnv::new();
    period
        .bind(sym("start_date"), Value::Date(date(2027, 2, 1)))
        .unwrap();
    period
        .bind(sym("end_date"), Value::Date(date(2027, 2, 2)))
        .unwrap();
    assert_eq!(
        render("days_between(start_date, end_date)", &period),
        "Given end_date = 2027-02-02, start_date = 2027-02-01; days_between(start_date, end_date) evaluates to 1."
    );
    assert_eq!(
        render("working_days([start_date, end_date], calendar, inclusive_end) * day_fraction", &hr_values()),
        "Given calendar = cal:PL-Office, day_fraction = 0.5, end_date = 2027-02-01, inclusive_end = true, start_date = 2027-02-01; working_days([start_date, end_date], calendar, inclusive_end) * day_fraction evaluates to 0.5 working_day."
    );
    assert_eq!(
        render("LeaveRequest.status == Approved -> exists(ApprovalRecord)", &hr_values()),
        "Given ApprovalRecord = <missing>, LeaveRequest.status = Approved; LeaveRequest.status == Approved -> exists(ApprovalRecord) evaluates to false."
    );
    // Hotfix 039: a contextual enum constant is never a Given input.
    let annual = render("leave_type == Annual", &hr_values());
    assert_eq!(
        annual,
        "Given leave_type = Annual; leave_type == Annual evaluates to true."
    );
    assert!(!annual.contains("Annual = <missing>"));
    assert!(!annual.contains("Given Annual"));
    let sick = render(
        "if leave_type in [Annual, Sick] then RequestedWorkingDays else 0",
        &hr_values(),
    );
    assert_eq!(
        sick,
        "Given RequestedWorkingDays = 2 working_day, leave_type = Annual; if leave_type in [Annual, Sick] then RequestedWorkingDays else 0 evaluates to 2 working_day."
    );
    // A known runtime input without a value is still listed as <missing>.
    let absent = render("exists(ApprovalRecord)", &hr_values());
    assert_eq!(
        absent,
        "Given ApprovalRecord = <missing>; exists(ApprovalRecord) evaluates to false."
    );
    // A root binding wins over an enum member of the same spelling elsewhere.
    let mut shadow_types = hr_types();
    shadow_types.bind_root(sym("Sick"), Ty::Bool).unwrap();
    let mut shadow_values = ValueEnv::new();
    shadow_values.bind(sym("Sick"), Value::Bool(true)).unwrap();
    let ast = parse("Sick or false").unwrap();
    let result = run("Sick or false", &shadow_types, &shadow_values).unwrap();
    assert_eq!(
        render_example(&ast, &shadow_types, &shadow_values, &result),
        "Given Sick = true; Sick or false evaluates to true."
    );
    let ast = parse("1 + 2.50").unwrap();
    assert_eq!(
        render_example(
            &ast,
            &TypeEnv::new(),
            &ValueEnv::new(),
            &Value::Decimal(dec("3.50"))
        ),
        "1 + 2.50 evaluates to 3.50."
    );
    // Value display conventions.
    for (value, text) in [
        (Value::String("a\"b\n".into()), "\"a\\\"b\\n\""),
        (
            Value::DateTime(Timestamp::from_str("2027-01-20T10:00:00+01:00").unwrap()),
            "2027-01-20T09:00:00.000000000Z",
        ),
        (
            Value::Duration {
                value: dec("2.50"),
                unit: u("hour"),
            },
            "2.50 hour",
        ),
        (
            Value::List(vec![Value::Int(1), Value::Bool(false)]),
            "[1, false]",
        ),
        (person("person:E100"), "person:E100"),
    ] {
        assert_eq!(value.to_string(), text);
    }
}

// ---------------------------------------------------------------- guards

#[test]
fn eval_source_guard() {
    let sources = [
        ("lib.rs", include_str!("../src/lib.rs")),
        ("typecheck.rs", include_str!("../src/typecheck.rs")),
        ("eval.rs", include_str!("../src/eval.rs")),
        ("calendar.rs", include_str!("../src/calendar.rs")),
        ("render.rs", include_str!("../src/render.rs")),
    ];
    for (name, source) in sources {
        for forbidden in [
            "std::fs",
            "File::open",
            "std::process",
            "Command",
            "reqwest",
            "TcpStream",
            "SystemClock",
            "Clock",
            "Utc::now",
            "now_utc",
            "Instant::now",
            "ArtifactStore",
            "RevisionStore",
            "Sqlite",
            "Graph",
            "NodePayload",
            "SemanticPatch",
            "InferenceProvider",
            "MockProvider",
            "libloading",
            "unsafe",
            "std::env",
            "plumb_psg",
            "plumb_functional",
            "plumb_validation",
            "plumb_patch",
            "plumb_store",
            "plumb_inference",
            "call_by_name",
            "invoke",
            "Value::Null",
            "Ty::Null",
            "Null(",
            "Null {",
            "Ty::Optional",
            "Optional(",
            "Ty::Any",
            "Value::Any",
            "f64",
            "f32",
            "as_of(",
            "Europe",
            "serde_yaml",
            "fixture",
            "cal:PL",
            "Approved",
            "Annual",
            "LeaveRequest",
        ] {
            assert!(!source.contains(forbidden), "{name} contains {forbidden}");
        }
    }
    // The parser modules are unchanged in S2.5 and still contain no execution.
    assert!(!include_str!("../src/parser.rs").contains("crate::eval"));
}

// ---------------------------------------------------------------- properties

/// Environment for generated expressions.
fn generated_env() -> (TypeEnv, ValueEnv) {
    scalar_env()
}

fn int_expr(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("i".to_owned()),
        Just("j".to_owned()),
        (0i64..50).prop_map(|v| v.to_string()),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    prop_oneof![
        leaf,
        (int_expr(depth - 1), int_expr(depth - 1)).prop_map(|(a, b)| format!("({a} + {b})")),
        (int_expr(depth - 1), int_expr(depth - 1)).prop_map(|(a, b)| format!("({a} - {b})")),
        (int_expr(depth - 1), 0i64..6).prop_map(|(a, k)| format!("({a} * {k})")),
        (int_expr(depth - 1), 1i64..6).prop_map(|(a, k)| format!("({a} % {k})")),
        (
            bool_expr(depth - 1),
            int_expr(depth - 1),
            int_expr(depth - 1)
        )
            .prop_map(|(c, a, b)| format!("(if {c} then {a} else {b})")),
    ]
    .boxed()
}

fn num_expr(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("d".to_owned()),
        Just("e".to_owned()),
        (0i64..1000, 1u32..3).prop_map(|(m, s)| Decimal::new(m, s).to_string()),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    prop_oneof![
        leaf,
        int_expr(depth - 1),
        (num_expr(depth - 1), num_expr(depth - 1)).prop_map(|(a, b)| format!("({a} + {b})")),
        (num_expr(depth - 1), int_expr(depth - 1)).prop_map(|(a, b)| format!("({a} - {b})")),
        (
            num_expr(depth - 1),
            prop_oneof![Just("2"), Just("0.5"), Just("1.25")]
        )
            .prop_map(|(a, k)| format!("({a} * {k})")),
        (
            num_expr(depth - 1),
            prop_oneof![Just("2"), Just("0.5"), Just("4")]
        )
            .prop_map(|(a, k)| format!("({a} / {k})")),
    ]
    .boxed()
}

fn qty_expr(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("h1".to_owned()),
        Just("h2".to_owned()),
        (0i64..100).prop_map(|v| format!("quantity({v}, hour)")),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    prop_oneof![
        leaf,
        (qty_expr(depth - 1), qty_expr(depth - 1)).prop_map(|(a, b)| format!("({a} + {b})")),
        (qty_expr(depth - 1), qty_expr(depth - 1)).prop_map(|(a, b)| format!("({a} - {b})")),
        (qty_expr(depth - 1), prop_oneof![Just("2"), Just("0.5")])
            .prop_map(|(a, k)| format!("({a} * {k})")),
        (qty_expr(depth - 1), prop_oneof![Just("2"), Just("4")])
            .prop_map(|(a, k)| format!("({a} / {k})")),
        (bool_expr(depth - 1), qty_expr(depth - 1))
            .prop_map(|(c, a)| format!("(if {c} then {a} else 0)")),
    ]
    .boxed()
}

fn bool_expr(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("b".to_owned()),
        Just("f".to_owned()),
        Just("true".to_owned()),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    let cmp = prop_oneof![
        Just("<"),
        Just("<="),
        Just(">"),
        Just(">="),
        Just("=="),
        Just("!=")
    ];
    prop_oneof![
        leaf,
        (num_expr(depth - 1), cmp.clone(), num_expr(depth - 1))
            .prop_map(|(a, op, b)| format!("({a} {op} {b})")),
        (qty_expr(depth - 1), cmp.clone(), qty_expr(depth - 1))
            .prop_map(|(a, op, b)| format!("({a} {op} {b})")),
        (qty_expr(depth - 1), cmp).prop_map(|(a, op)| format!("({a} {op} 0)")),
        (bool_expr(depth - 1), bool_expr(depth - 1)).prop_map(|(a, b)| format!("({a} and {b})")),
        (bool_expr(depth - 1), bool_expr(depth - 1)).prop_map(|(a, b)| format!("({a} or {b})")),
        (bool_expr(depth - 1), bool_expr(depth - 1)).prop_map(|(a, b)| format!("({a} -> {b})")),
        bool_expr(depth - 1).prop_map(|a| format!("(not {a})")),
        (
            int_expr(depth - 1),
            int_expr(depth - 1),
            int_expr(depth - 1)
        )
            .prop_map(|(x, a, b)| format!("({x} in [{a}, {b}])")),
    ]
    .boxed()
}

fn well_typed() -> BoxedStrategy<String> {
    prop_oneof![int_expr(3), num_expr(3), qty_expr(3), bool_expr(3)].boxed()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn eval_property_well_typed_expressions_evaluate(src in well_typed()) {
        let (types, values) = generated_env();
        let ast = parse(&src).unwrap();
        let ty = typecheck(&ast, &types);
        prop_assert!(ty.is_ok(), "{} : {:?}", src, ty);
        let calendars = hr_calendar();
        let value = eval(&ast, &EvalContext { types: &types, values: &values, calendars: &calendars, predicates: &Managers });
        prop_assert!(value.is_ok(), "{} => {:?}", src, value);
        // Deterministic: the same inputs give the same value and rendering.
        let again = eval(&ast, &EvalContext { types: &types, values: &values, calendars: &calendars, predicates: &Managers });
        prop_assert_eq!(&value, &again);
        let v = value.unwrap();
        prop_assert_eq!(render_example(&ast, &types, &values, &v), render_example(&ast, &types, &values, &v));
    }

    #[test]
    fn eval_property_unit_mismatch_is_rejected(a in qty_expr(2), op in prop_oneof![Just("+"), Just("-"), Just("=="), Just("<")]) {
        let (types, _) = generated_env();
        let src = format!("({a}) {op} m1");
        prop_assert!(typecheck(&parse(&src).unwrap(), &types).is_err(), "{}", src);
    }

    #[test]
    fn eval_property_working_days_bounded_by_days_between(start in 0i64..800, length in 0i64..60) {
        let types = hr_types();
        let first = date(2026, 1, 1) + time::Duration::days(start);
        let last = first + time::Duration::days(length);
        let mut values = ValueEnv::new();
        values.bind(sym("start_date"), Value::Date(first)).unwrap();
        values.bind(sym("end_date"), Value::Date(last)).unwrap();
        values.bind(sym("calendar"), record(CALENDAR, CAL, vec![])).unwrap();
        let count = |src: &str| match run(src, &types, &values).unwrap() {
            Value::Quantity { value, .. } => value,
            Value::Int(i) => Decimal::from(i),
            other => panic!("{other:?}"),
        };
        let between = count("days_between(start_date, end_date)");
        prop_assert!(count("working_days([start_date, end_date], calendar, true)") <= between + Decimal::ONE);
        prop_assert!(count("working_days([start_date, end_date], calendar, false)") <= between);
    }

    #[test]
    fn eval_property_short_circuit_needs_no_unselected_value(c in bool_expr(1)) {
        let (types, values) = generated_env();
        for src in [
            format!("(f and missing == 1) or {c}"),
            format!("if true then {c} else missing == 1"),
            format!("({c} or true) or missing == 1"),
            "f -> (missing == 1)".to_owned(),
        ] {
            let result = run(&src, &types, &values);
            prop_assert!(result.is_ok(), "{} => {:?}", src, result);
        }
    }
}
