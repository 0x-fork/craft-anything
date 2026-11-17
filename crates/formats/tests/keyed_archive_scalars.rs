//! Regression tests for `NSString` (`NS.bytes`) and `NSDate` (`NS.time`) projection in `keyed_archive`.

use craft_anything_formats::keyed_archive::{KeyedArchive, from_plist, parse};
use craft_anything_formats::plist::Value;
use craft_anything_testkit::apple as fx;
use serde_json::json;

/// Build a `$class` dictionary entry in `$objects`.
fn class_dict(name: &str) -> Value {
    Value::Dict(vec![("$classname".into(), Value::String(name.into()))])
}

/// Inline `NS.string` and `Value::Real` `NS.time` continue to project as expected.
#[test]
fn inline_nsstring_and_real_nsdate_continue_to_project() {
    let archive = fx::keyed_archive();
    let parsed = parse(&archive).unwrap();
    let (_, root) = parsed.root(None).unwrap();
    assert_eq!(parsed.project(root)["title"], "Groceries");
}

/// `NSString` / `NSMutableString` whose `NS.bytes` is a UID or `CF$UID` reference into `$objects` projects as a string.
#[test]
fn indirect_uid_nsbytes_projects_to_utf8_string_in_binary_and_xml() {
    let plist_val = Value::Dict(vec![
        ("$archiver".into(), Value::String("NSKeyedArchiver".into())),
        ("$version".into(), Value::Integer(100_000)),
        ("$top".into(), Value::Dict(vec![("root".into(), Value::Uid(1))])),
        (
            "$objects".into(),
            Value::Array(vec![
                Value::String("$null".into()),
                Value::Dict(vec![("$class".into(), Value::Uid(2)), ("greeting".into(), Value::Uid(3)), ("mutable".into(), Value::Uid(5))]),
                class_dict("Envelope"),
                Value::Dict(vec![("$class".into(), Value::Uid(4)), ("NS.bytes".into(), Value::Uid(6))]),
                class_dict("NSString"),
                Value::Dict(vec![("$class".into(), Value::Uid(7)), ("NS.bytes".into(), Value::Uid(6))]),
                Value::Data("héllo 🌍".as_bytes().to_vec()),
                class_dict("NSMutableString"),
            ]),
        ),
    ]);

    let archive = from_plist(&plist_val).unwrap();
    let projected = archive.project(1);
    assert_eq!(projected["greeting"], json!("héllo 🌍"), "{projected}");
    assert_eq!(projected["mutable"], json!("héllo 🌍"), "{projected}");

    // XML keyed-archive form uses `{"CF$UID": n}` dictionaries instead of binary UIDs.
    let xml = fx::to_xml(&fx::dict(vec![
        ("$archiver", fx::s("NSKeyedArchiver")),
        ("$version", fx::P::Int(100_000)),
        ("$top", fx::dict(vec![("root", fx::P::Uid(1))])),
        (
            "$objects",
            fx::P::Array(vec![
                fx::s("$null"),
                fx::dict(vec![("$class", fx::P::Uid(2)), ("greeting", fx::P::Uid(3)), ("mutable", fx::P::Uid(5))]),
                fx::dict(vec![("$classname", fx::s("Envelope"))]),
                fx::dict(vec![("$class", fx::P::Uid(4)), ("NS.bytes", fx::P::Uid(6))]),
                fx::dict(vec![("$classname", fx::s("NSString"))]),
                fx::dict(vec![("$class", fx::P::Uid(7)), ("NS.bytes", fx::P::Uid(6))]),
                fx::P::Data("héllo 🌍".as_bytes().to_vec()),
                fx::dict(vec![("$classname", fx::s("NSMutableString"))]),
            ]),
        ),
    ]));
    let xml_archive = parse(xml.as_bytes()).unwrap();
    let xml_projected = xml_archive.project(1);
    assert_eq!(xml_projected["greeting"], json!("héllo 🌍"), "{xml_projected}");
    assert_eq!(xml_projected["mutable"], json!("héllo 🌍"), "{xml_projected}");
}

/// `NSDate` with integer `NS.time` or UID-referenced `NS.time` projects to `{"$date": ...}`.
#[test]
fn integer_and_uid_referenced_nstime_project_to_iso_dates() {
    let archive = KeyedArchive {
        archiver: "NSKeyedArchiver".into(),
        version: Some(100_000),
        top: vec![("root".into(), 1)],
        objects: vec![
            Value::String("$null".into()),
            Value::Dict(vec![
                ("$class".into(), Value::Uid(2)),
                ("epoch_int".into(), Value::Uid(3)),
                ("uid_real".into(), Value::Uid(5)),
                ("uid_int".into(), Value::Uid(7)),
            ]),
            class_dict("Timestamps"),
            Value::Dict(vec![("$class".into(), Value::Uid(4)), ("NS.time".into(), Value::Integer(0))]),
            class_dict("NSDate"),
            Value::Dict(vec![("$class".into(), Value::Uid(4)), ("NS.time".into(), Value::Uid(6))]),
            Value::Real(86_400.5),
            Value::Dict(vec![("$class".into(), Value::Uid(4)), ("NS.time".into(), Value::Uid(8))]),
            Value::Integer(86_400),
        ],
    };

    let projected = archive.project(1);
    assert_eq!(projected["epoch_int"], json!({"$date": "2001-01-01T00:00:00Z"}), "{projected}");
    assert_eq!(projected["uid_real"], json!({"$date": "2001-01-02T00:00:00.5Z"}), "{projected}");
    assert_eq!(projected["uid_int"], json!({"$date": "2001-01-02T00:00:00Z"}), "{projected}");
}

/// Repeated `NS.bytes` expansions respect the shared `MAX_REFERENCED_BYTES` projection budget.
#[test]
fn repeated_nsbytes_expansions_are_bounded_by_projection_byte_budget() {
    // 1 MiB payload referenced by 80 distinct `NSString` wrappers (80 MiB > 64 MiB budget).
    let chunk = vec![b'x'; 1024 * 1024];
    let mut objects = vec![Value::String("$null".into()), class_dict("NSArray"), class_dict("NSString"), Value::Data(chunk)];
    let mut item_uids = Vec::new();
    for i in 0..80u64 {
        let uid = objects.len() as u64;
        objects.push(Value::Dict(vec![("$class".into(), Value::Uid(2)), ("NS.bytes".into(), Value::Uid(3))]));
        let _ = i;
        item_uids.push(Value::Uid(uid));
    }
    let root_uid = objects.len() as u64;
    objects.push(Value::Dict(vec![("$class".into(), Value::Uid(1)), ("NS.objects".into(), Value::Array(item_uids))]));

    let archive = KeyedArchive { archiver: "NSKeyedArchiver".into(), version: Some(100_000), top: vec![("root".into(), root_uid)], objects };
    let projected = archive.project(root_uid);
    let items = projected["items"].as_array().unwrap();
    assert_eq!(items.len(), 80);
    assert!(items[0].is_string());
    assert!(items.iter().any(|it| it["$note"] == "projection byte limit"), "expected byte budget cap to stop unbounded NS.bytes expansion");
}
