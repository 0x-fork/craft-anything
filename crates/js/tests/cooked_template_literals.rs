//! Regression tests for cooked template-literal values across JS fact and bundle extraction.

use craft_anything_js::{Limits, analyze};

/// `require(...)`, `(require)(...)`, computed member chains, `await import(...)`, and `require("bindings")(...)`
/// decode escaped no-substitution template literals so module aliases, effects, IPC, and native modules survive.
#[test]
fn require_and_dynamic_import_decode_cooked_templates_and_propagate_aliases() {
    let src = r#"
        const fs = require(`node\x3afs`);
        fs.readFileSync(`/etc/hosts`);
        const ipc = (require)(`elec\x74ron`)[`ipc\x52enderer`];
        ipc[`in\x76oke`](`note\x3asave`);
        async function run() {
            const cp = await import(`node\x3achild_process`);
            cp.exec(`id`);
        }
        require(`bind\x69ngs`)(`native\x2daddon`);
    "#;

    let a = analyze("main.js", src, &Limits::default());
    let specs: Vec<&str> = a.facts.imports.iter().map(|i| i.specifier.as_str()).collect();
    assert!(specs.contains(&"node:fs"), "imports: {specs:?}");
    assert!(specs.contains(&"electron"), "imports: {specs:?}");
    assert!(specs.contains(&"node:child_process"), "imports: {specs:?}");

    let effect_apis: Vec<&str> = a.facts.effects.iter().map(|e| e.api.as_str()).collect();
    assert!(effect_apis.contains(&"fs.readFileSync"), "effects: {effect_apis:?}");
    assert!(effect_apis.contains(&"child_process.exec"), "effects: {effect_apis:?}");

    assert!(a.facts.ipc.iter().any(|i| i.side == "renderer" && i.method == "invoke" && i.channel.as_deref() == Some("note:save")), "ipc: {:?}", a.facts.ipc);
    assert!(a.facts.native_modules.contains(&"bindings:native-addon".to_string()), "native_modules: {:?}", a.facts.native_modules);
}

/// Routes, XHR methods, `contextBridge` keys, `Object.defineProperty(exports, ...)`, and computed CJS exports
/// use cooked template text rather than raw escape spellings.
#[test]
fn arg_str_consumers_decode_escaped_templates_for_routes_xhr_ipc_and_exports() {
    let src = r#"
        app.get(`\x2fapi\x2fnotes`, function h() {});
        const routerConfig = [{ path: `\x2fdashboard` }];
        xhr.open(`P\x4fST`, `/api/submit`);
        contextBridge.exposeInMainWorld(`app\x41pi`, { save() {} });
        Object.defineProperty(exports, `my\x45xport`, { value: 1 });
        Object.defineProperty(exports, `__es\x4dodule`, { value: true });
        exports[`other\x45xport`] = 2;
    "#;

    let a = analyze("preload.cjs", src, &Limits::default());
    assert!(a.facts.routes.iter().any(|r| r.path == "/api/notes" && r.method.as_deref() == Some("GET")), "routes: {:?}", a.facts.routes);
    assert!(a.facts.routes.iter().any(|r| r.path == "/dashboard" && r.source == "router-config"), "routes: {:?}", a.facts.routes);
    assert!(
        a.facts.endpoints.iter().any(|e| e.api == "xhr" && e.method.as_deref() == Some("POST") && e.url == "/api/submit"),
        "endpoints: {:?}",
        a.facts.endpoints
    );
    assert!(
        a.facts.exposed.iter().any(|x| x.key.as_deref() == Some("appApi") && x.members.iter().any(|m| m.function.as_deref() == Some("appApi.save"))),
        "exposed: {:?}",
        a.facts.exposed
    );
    assert!(a.facts.exports.cjs.contains(&"myExport".to_string()), "cjs exports: {:?}", a.facts.exports.cjs);
    assert!(a.facts.exports.cjs.contains(&"otherExport".to_string()), "cjs exports: {:?}", a.facts.exports.cjs);
    assert!(!a.facts.exports.cjs.iter().any(|e| e.contains("esModule") || e.contains("es\\x4dodule")), "cjs exports: {:?}", a.facts.exports.cjs);

    let tsx_src = r#"
        self[`webpackChunk\x61pp`] = [];
        const doc = { kind: `Operation\x44efinition`, operation: `qu\x65ry`, name: { value: `Get\x43ompiled` } };
        const router = createTRPCRouter({ [`by\x49d`]: publicProcedure.query(fn) });
        export const view = <Route path={`\x2fsettings`} />;
    "#;
    let tsx = analyze("routes.tsx", tsx_src, &Limits::default());
    assert!(tsx.facts.routes.iter().any(|r| r.path == "/settings" && r.source == "jsx"), "jsx routes: {:?}", tsx.facts.routes);
    assert!(tsx.facts.bundler_markers.contains_key("webpackChunk"), "markers: {:?}", tsx.facts.bundler_markers);
    assert!(
        tsx.facts.api_operations.iter().any(|op| op.protocol == "graphql" && op.kind == "query" && op.name.as_deref() == Some("GetCompiled")),
        "api ops: {:?}",
        tsx.facts.api_operations
    );
    assert!(
        tsx.facts.api_operations.iter().any(|op| op.protocol == "trpc" && op.kind == "query" && op.name.as_deref() == Some("byId")),
        "api ops: {:?}",
        tsx.facts.api_operations
    );
}

/// Webpack chunk IDs, webpack module-loader calls, and browserify dependency maps accept no-substitution template literals.
#[test]
fn bundle_loaders_and_chunk_ids_accept_cooked_template_literals() {
    let webpack_src = r#"
        (self.webpackChunkapp = self.webpackChunkapp || []).push([
            [`chunk\x2dmain`],
            {
                "./src/index.js": function(m, e, req) {
                    const dep = req(`./dep\x2ejs`);
                    m.exports = dep;
                },
                "./dep.js": function(m) {
                    m.exports = 1;
                }
            }
        ]);
    "#;
    let wp = analyze("chunk.js", webpack_src, &Limits::default());
    assert_eq!(wp.bundle.chunk_ids, vec!["chunk-main"]);
    let entry = wp.bundle.modules.iter().find(|m| m.id == "./src/index.js").expect("entry module");
    assert_eq!(entry.deps, vec!["./dep.js"]);

    let browserify_src = r#"
        (function e(t,n,r){})({
            "1": [function(require, module, exports) { require("./dep"); }, { "./dep": `2` }],
            "2": [function(require, module, exports) { module.exports = 42; }, {}]
        },{},[1]);
    "#;
    let bf = analyze("bf.js", browserify_src, &Limits::default());
    let m1 = bf.bundle.modules.iter().find(|m| m.id == "1").expect("browserify module 1");
    assert_eq!(m1.deps, vec!["2"]);
}

/// Untagged template literals containing invalid escapes (`cooked: None`) are not reported as literal raw strings.
#[test]
fn invalid_escape_untagged_templates_are_not_reported_as_raw_literals() {
    let src = r#"
        require(`./bad\8name`);
        fetch(`https://ex.test/\xZZ`);
    "#;

    let a = analyze("broken.js", src, &Limits::default());
    assert!(
        !a.facts.imports.iter().any(|i| i.literal && i.specifier.contains("bad")),
        "invalid template must not produce literal import: {:?}",
        a.facts.imports
    );
    assert!(
        !a.facts.endpoints.iter().any(|e| e.literal && e.url.contains("ex.test")),
        "invalid template must not produce literal endpoint: {:?}",
        a.facts.endpoints
    );
    assert!(
        !a.facts.strings.iter().any(|s| s.contains("ex.test") || s.contains("bad")),
        "invalid template must not enter literal strings: {:?}",
        a.facts.strings
    );
}

/// Ordinary string literals, interpolated templates, and ES2018 tagged GraphQL templates (including raw escapes where `cooked: None`) retain existing extraction behavior.
#[test]
fn tagged_gql_and_plain_string_literals_control() {
    let src = "
        const fs = require(\"node:fs\");
        fetch(`/api/items/${itemId}`);
        const DOC = gql`query GetItems { items { id } }`;
        const RAW_DOC = gql`# raw escape \\xZZ allowed in ES2018 tagged template
query GetRawItems { items { id } }`;
    ";

    let a = analyze("control.js", src, &Limits::default());
    assert!(a.facts.imports.iter().any(|i| i.specifier == "node:fs" && i.literal));
    assert!(a.facts.endpoints.iter().any(|e| e.api == "fetch" && e.url == "/api/items/{itemId}" && !e.literal));
    assert!(a.facts.api_operations.iter().any(|op| op.protocol == "graphql" && op.name.as_deref() == Some("GetItems")));
    assert!(a.facts.api_operations.iter().any(|op| op.protocol == "graphql" && op.name.as_deref() == Some("GetRawItems")), "ops: {:?}", a.facts.api_operations);
}
