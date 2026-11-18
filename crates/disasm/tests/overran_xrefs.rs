//! Regressions for stale cross-references and quota consumption left behind when overran
//! functions are re-scanned after later function boundaries are discovered.

use craft_anything_disasm::{Image, Limits, NoControl, Program, XrefKind};
use craft_anything_formats::Arch;
use std::sync::Arc;

/// Verify that a re-scanned switch function drops jump xrefs that pointed past the next function.
#[test]
fn switch_overrun_drops_stale_jump_xrefs_after_rescan() {
    // 0x1000: call 0x1080; jmp [rax*8 + 0x1100]      cases: 0x1010, 0x1011 (ret each)
    // 0x1080: 8 nops; ret                            table: 0x1010, 0x1011, 0x1085, 0x1088, 0
    let mut code = vec![0xcc; 0x140];
    code[..12].copy_from_slice(&[0xe8, 0x7b, 0, 0, 0, 0xff, 0x24, 0xc5, 0x00, 0x11, 0, 0]);
    code[0x10..0x12].copy_from_slice(&[0xc3, 0xc3]);
    code[0x80..0x89].copy_from_slice(&[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xc3]);
    for (i, t) in [0x1010u64, 0x1011, 0x1085, 0x1088, 0].iter().enumerate() {
        code[0x100 + i * 8..0x108 + i * 8].copy_from_slice(&t.to_le_bytes());
    }
    let raw = Image::raw(Arc::from(code), Arch::X86_64, 0x1000);
    let p = Program::analyze(&raw, &Limits::default(), &NoControl).unwrap();

    let caller = p.functions.get(&0x1000).unwrap();
    assert_eq!(caller.jump_tables.first().map(|t| t.targets.clone()), Some(vec![0x1010, 0x1011]));

    let outgoing: Vec<u64> = p.xrefs_from.get(&0x1005).map(|xs| xs.iter().map(|x| x.to).collect()).unwrap_or_default();
    assert_eq!(outgoing, vec![0x1010, 0x1011], "stale jump targets remained in xrefs_from: {outgoing:#x?}");
    assert!(!p.xrefs_to.contains_key(&0x1085), "stale xref into 0x1085: {:?}", p.xrefs_to.get(&0x1085));
    assert!(!p.xrefs_to.contains_key(&0x1088), "stale xref into 0x1088: {:?}", p.xrefs_to.get(&0x1088));
    assert!(p.callers(0x1085).is_empty(), "stale caller remained for 0x1085: {:?}", p.callers(0x1085));
}

/// Verify that stale xrefs emitted inside unreachable code picked up only by an overran switch
/// scan are removed after trimming, including calls and data references.
#[test]
fn trimmed_overrun_block_drops_call_and_data_xrefs() {
    // 0x1000: call 0x1080; jmp [rax*8 + 0x1100]      cases: 0x1010, 0x1011 (ret)
    // 0x1080: ret                                    (one byte: body stops at 0x1081)
    // 0x1090: lea rdi, [rip+0x19] (0x10b0); ret      (only reachable if leaked switch table runs past 0x1080)
    // 0x10b0: "HelloWorld"
    // table at 0x1100: [0x1010, 0x1011, 0x1090, 0]
    let mut code = vec![0xcc; 0x140];
    code[..12].copy_from_slice(&[0xe8, 0x7b, 0, 0, 0, 0xff, 0x24, 0xc5, 0x00, 0x11, 0, 0]);
    code[0x10..0x12].copy_from_slice(&[0xc3, 0xc3]);
    code[0x80] = 0xc3;
    // 0x1090: lea rdi, [rip + 0x19] -> rip is 0x1097, target 0x10b0; ret
    code[0x90..0x98].copy_from_slice(&[0x48, 0x8d, 0x3d, 0x19, 0x00, 0x00, 0x00, 0xc3]);
    code[0xb0..0xba].copy_from_slice(b"HelloWorld");
    for (i, t) in [0x1010u64, 0x1011, 0x1090, 0].iter().enumerate() {
        code[0x100 + i * 8..0x108 + i * 8].copy_from_slice(&t.to_le_bytes());
    }
    let raw = Image::raw(Arc::from(code), Arch::X86_64, 0x1000);
    let p = Program::analyze(&raw, &Limits::default(), &NoControl).unwrap();

    assert!(p.function_containing(0x1090).is_none(), "0x1090 should not stay attached to any function");
    assert!(!p.xrefs_from.contains_key(&0x1090), "stale data/string xref from dropped overrun block: {:?}", p.xrefs_from.get(&0x1090));
    assert!(!p.xrefs_to.contains_key(&0x10b0), "stale string xref target: {:?}", p.xrefs_to.get(&0x10b0));
    assert!(!p.xrefs_to.contains_key(&0x1090), "stale jump xref into 0x1090: {:?}", p.xrefs_to.get(&0x1090));
}

/// Verify that stale xrefs produced before an overran re-scan do not exhaust `limits.max_xrefs`.
#[test]
fn overran_rescan_frees_xref_budget_for_other_functions() {
    // 0x1000: call 0x1080; jmp [rax*8 + 0x1100]      valid cases: 0x1010, 0x1011 (3 final xrefs: 1 call + 2 jumps)
    // 0x1080: call 0x10c0; ret                       (1 call xref)
    // 0x10c0: ret
    // table at 0x1100: [0x1010, 0x1011, 0x1080, 0x1085, 0x1090, 0x10a0, 0]
    // First pass on 0x1000 emits 7 xrefs (1 call + 6 switch jumps) before re-scan trims 0x1000 to 3 xrefs.
    // With max_xrefs = 4, the caller keeps its 3 trimmed xrefs and 0x1080 still keeps its call xref.
    let mut code = vec![0xcc; 0x140];
    code[..12].copy_from_slice(&[0xe8, 0x7b, 0, 0, 0, 0xff, 0x24, 0xc5, 0x00, 0x11, 0, 0]);
    code[0x10..0x12].copy_from_slice(&[0xc3, 0xc3]);
    // 0x1080: call 0x10c0; ret
    code[0x80..0x86].copy_from_slice(&[0xe8, 0x3b, 0x00, 0x00, 0x00, 0xc3]);
    code[0x86..0xa8].fill(0x90);
    code[0xa8] = 0xc3;
    code[0xc0] = 0xc3;
    for (i, t) in [0x1010u64, 0x1011, 0x1080, 0x1085, 0x1090, 0x10a0, 0].iter().enumerate() {
        code[0x100 + i * 8..0x108 + i * 8].copy_from_slice(&t.to_le_bytes());
    }
    let raw = Image::raw(Arc::from(code), Arch::X86_64, 0x1000);
    let limits = Limits { max_xrefs: 4, ..Limits::default() };
    let p = Program::analyze(&raw, &limits, &NoControl).unwrap();

    let callee_call = p.xrefs_from.get(&0x1080).and_then(|xs| xs.iter().find(|x| x.kind == XrefKind::Call && x.to == 0x10c0));
    assert!(callee_call.is_some(), "0x1080 lost its call xref because stale overrun xrefs spent the budget: {:?}", p.xrefs_from);
    assert_eq!(p.stats.xrefs, 4);
}
