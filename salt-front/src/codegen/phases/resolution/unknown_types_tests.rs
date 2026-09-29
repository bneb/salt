//! Pins which type names `unknown_types::check` accepts, which it reports,
//! and where it reports them.
#[cfg(test)]
mod tests {
    use crate::codegen::phases::resolution::unknown_types::check;
    use crate::grammar::SaltFile;

    fn parse(source: &str) -> SaltFile {
        syn::parse_str(&crate::preprocess(source)).expect("test source parses")
    }

    /// Each reported line up to its explanation: "unknown type `X` in <site>".
    fn unknown(source: &str, loaded: &[&str]) -> Vec<String> {
        let loaded: Vec<SaltFile> = loaded.iter().map(|s| parse(s)).collect();
        match check(&parse(source), &loaded) {
            Ok(()) => Vec::new(),
            Err(message) => message.lines().map(|l| l.split(':').next().unwrap().to_string()).collect(),
        }
    }

    #[test]
    fn reports_each_unknown_type_once_where_first_used() {
        let source = "fn f(a: Box<i32>) -> Box<i32> {\n    return a;\n}\nfn g(b: Frob, c: Box<i64>) -> i32 {\n    return 0;\n}\n";
        let loaded: Vec<SaltFile> = Vec::new();
        let why = "not declared in this file, in any module it imports, or in the prelude";
        assert_eq!(
            check(&parse(source), &loaded),
            Err(format!("unknown type `Box` in fn `f`: {}\nunknown type `Frob` in fn `g`: {}", why, why))
        );
    }

    #[test]
    fn generic_parameters_are_known_only_in_their_scope() {
        let source = "
struct W<Item> { v: Item }
enum E<Item> { A(Item), B }
fn id<Val>(x: Val) -> Val {
    return x;
}
struct Buf<const SIZE: i64> { n: i64 }
fn take<const SIZE: i64>(b: Buf<SIZE>) -> i64 {
    return 0;
}
impl<Item> W<Item> {
    fn get(self) -> Item {
        return self.v;
    }
    fn pick<Other>(self, u: Other, w: Self) -> Other {
        return u;
    }
}
trait Tr<Item> {
    fn m<Other>(x: Item, y: Other) -> Other;
}
fn after(x: Item, y: Other, n: SIZE, v: Val) -> i32 {
    return 0;
}
";
        let after = |name: &str| format!("unknown type `{}` in fn `after`", name);
        assert_eq!(unknown(source, &[]), [after("Item"), after("Other"), after("SIZE"), after("Val")]);
    }

    #[test]
    fn an_undeclared_capital_letter_is_an_implicit_generic() {
        let source = "struct Pool<T> { n: i64 }\nimpl Pool<T> {\n    fn put(self, x: T) -> T {\n        return x;\n    }\n}\n";
        assert_eq!(unknown(source, &[]), Vec::<String>::new());
    }

    /// One unknown name per checked position, A1 to A18 in visiting order.
    const EVERY_POSITION: &str = "
struct Wrap<T> { v: T }
struct S { a: A1 }
enum E { V(A2) }
extern fn ext(x: A3) -> A4;
global G: A5 = 0;
const C: A6 = 0;
impl A7 {
    fn m(self, x: A8) -> i32 {
        return 0;
    }
}
trait Tr {
    fn sig(x: A9) -> i32;
    fn dflt(x: A10) -> i32 {
        return 0;
    }
}
concept Pos {
    requires(v: A11) { v > 0 }
}
fn nested(p: Ptr<A12>, r: &A13, a: [A14; 2], t: (i32, A15), f: fn(A16) -> A17, w: Wrap<A18>) -> i32 {
    return 0;
}
";

    #[test]
    fn checks_every_item_level_type_position() {
        let site = |n: usize, at: &str| format!("unknown type `A{}` in {}", n, at);
        let mut expected = vec![
            site(1, "struct `S`"), site(2, "enum `E`"), site(3, "extern fn `ext`"), site(4, "extern fn `ext`"),
            site(5, "global `G`"), site(6, "const `C`"), site(7, "impl block"), site(8, "fn `m`"),
            site(9, "trait `Tr`"), site(10, "fn `dflt`"), site(11, "concept `Pos`"),
        ];
        expected.extend((12..=18).map(|n| site(n, "fn `nested`")));
        assert_eq!(unknown(EVERY_POSITION, &[]), expected);
    }

    #[test]
    fn types_a_loaded_module_declares_are_known() {
        let module = "package m\n\npub struct Frob<T> { v: T }\npub enum Mode { A, B }\ntrait Show {\n    fn show(x: i32) -> i32;\n}\nconcept Small {\n    requires(v: i32) { v < 10 }\n}\n";
        let source = "fn f(a: Frob<Mode>, b: Show, c: Small) -> i32 {\n    return 0;\n}\n";
        assert_eq!(unknown(source, &[module]), Vec::<String>::new());
        assert_eq!(unknown(source, &[]).len(), 4, "without the module, all four are unknown");
    }

    #[test]
    fn an_import_alias_is_known_when_the_item_it_renames_is() {
        let module = "package m\n\npub struct Frob { v: i32 }\n";
        let source = "use m.Frob as Fr;\nuse m.Gone as Gn;\nfn f(a: Fr, b: Gn) -> i32 {\n    return 0;\n}\n";
        assert_eq!(unknown(source, &[module]), ["unknown type `Gn` in fn `f`"]);
    }

    #[test]
    fn a_qualified_name_is_known_when_its_last_segment_is_declared() {
        let module = "package m\n\npub struct Frob { v: i32 }\n";
        let source = "fn f(a: m.Frob, b: m.Gone<i32>, c: missing.Thing, d: std.Owned<i32>) -> i32 {\n    return 0;\n}\n";
        assert_eq!(
            unknown(source, &[module]),
            ["unknown type `m.Gone` in fn `f`", "unknown type `missing.Thing` in fn `f`", "unknown type `std.Owned` in fn `f`"]
        );
    }

    #[test]
    fn types_the_compiler_lowers_itself_are_known() {
        let source = "fn f(a: i8, b: u64, c: usize, d: f32, e: bool, g: Owned<i32>, h: Atomic<i64>, k: Vector4f32, l: LlvmPtr) -> i32 {\n    return 0;\n}\n";
        assert_eq!(unknown(source, &[]), Vec::<String>::new());
    }

    #[test]
    fn window_takes_a_region_name_after_its_element_type() {
        let source = "fn f(w: Window<u8, RAM>, v: Window<Frob, VRAM>) -> i32 {\n    return 0;\n}\n";
        assert_eq!(unknown(source, &[]), ["unknown type `Frob` in fn `f`"]);
    }

    #[test]
    fn str_is_known_only_behind_a_reference() {
        let source = "fn f(a: &str, b: &mut str) -> i32 {\n    return 0;\n}\nfn g(c: str) -> i32 {\n    return 0;\n}\n";
        assert_eq!(unknown(source, &[]), ["unknown type `str` in fn `g`"]);
    }
}
