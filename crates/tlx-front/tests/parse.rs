//! Unit tests for CST -> surface AST lowering, one (or a few) per construct.
//! Shapes are compared through the compact s-expression printer
//! (`tlx_front::parse::sexp`).

use tlx_front::ast::*;
use tlx_front::parse::{parse_module, sexp};

fn module(body: &str) -> Module {
    let src = format!("---- MODULE T ----\n{body}\n====\n");
    match parse_module(&src, FileId(7)) {
        Ok(m) => m,
        Err(d) => panic!("parse failed for:\n{src}\n{d:#?}"),
    }
}

fn errors(body: &str) -> Vec<String> {
    let src = format!("---- MODULE T ----\n{body}\n====\n");
    match parse_module(&src, FileId(0)) {
        Ok(m) => panic!("expected failure, got {}", sexp::module(&m)),
        Err(d) => d.iter().map(|d| d.to_string()).collect(),
    }
}

/// s-expression of each unit of `body`.
fn units(body: &str) -> Vec<String> {
    module(body).units.iter().map(sexp::unit).collect()
}

/// s-expression of the expression `e` (as the body of a definition).
fn ex(e: &str) -> String {
    let m = module(&format!("X == {e}"));
    match &m.units[..] {
        [Unit::OpDef(d)] => sexp::expr(&d.body),
        _ => panic!("unexpected units for {e}"),
    }
}

fn body(m: &Module, i: usize) -> &Expr {
    match &m.units[i] {
        Unit::OpDef(d) => &d.body,
        _ => panic!("unit {i} is not an OpDef"),
    }
}

// ------------------------------------------------------------------ module

#[test]
fn module_header_extends_and_outside_text() {
    let src = "This is ignored.\n---- MODULE Foo ----\nEXTENDS Naturals, Sequences\n\
               X == 1\n=====\nignored too\n---- MODULE Bar ----\n====\n";
    let m = parse_module(src, FileId(3)).unwrap();
    assert_eq!(&*m.name.name, "Foo");
    assert_eq!(m.name.span.line, 2);
    assert_eq!(m.name.span.col, 13);
    assert_eq!(m.name.span.file, FileId(3));
    let ext: Vec<&str> = m.extends.iter().map(|i| &*i.name).collect();
    assert_eq!(ext, ["Naturals", "Sequences"]);
    assert_eq!(m.units.len(), 1);
}

#[test]
fn nested_module() {
    let u = units("---- MODULE Inner ----\nVARIABLE y\nI == y\n====\nY == 2");
    assert_eq!(u, ["(module Inner\n  (variables y)\n  (def I () y))", "(def Y () 2)"]);
}

#[test]
fn dropped_units() {
    let u = units(
        "X == 1\n----\nTHEOREM Thm == X = 1\n<1>1. X = 1 OBVIOUS\n<1> QED\n\
         LEMMA X = 1 BY DEF X\nCOROLLARY TRUE\nPROPOSITION TRUE\nUSE DEF X\nHIDE DEF X\n\
         (* --algorithm a begin skip; end algorithm *)\n\\* comment\nY == 2",
    );
    assert_eq!(u, ["(def X () 1)", "(theorem Thm (= X 1))", "(def Y () 2)"]);
    let u = units("THEOREM T == ASSUME NEW x PROVE x = x\nY == 2");
    assert_eq!(u, ["(def Y () 2)"]);
}

#[test]
fn multiple_top_level_modules() {
    let src = "---- MODULE A ----\nX == 1\n====\n---- MODULE B ----\nY == 2\n====\n";
    let ms = tlx_front::parse::parse_modules(src, Default::default()).unwrap();
    assert_eq!(ms.iter().map(|m| &*m.name.name).collect::<Vec<_>>(), ["A", "B"]);
}

// -------------------------------------------------------------- declarations

#[test]
fn constants_variables() {
    assert_eq!(
        units("CONSTANT a, b\nCONSTANTS F(_, _), _ ++ _, -. _, _ ^+\nVARIABLE x\nVARIABLES y, z"),
        ["(constants a b)", "(constants F/2 ++/2 -./1 ^+/1)", "(variables x)", "(variables y z)"]
    );
}

#[test]
fn operator_definitions() {
    assert_eq!(
        units(
            "F(x, G(_)) == G(x)\n-. a == a\na ++ b == a\na ^+ == a\nLOCAL L == 1\n\
             a \\prec b == a\na =< b == a\nNat == 1"
        ),
        [
            "(def F (x G/1) (G x))",
            "(def -. (a) a)",
            "(def ++ (a b) a)",
            "(def ^+ (a) a)",
            "(local-def L () 1)",
            "(def \\prec (a b) a)",
            "(def <= (a b) a)",
            "(def Nat () 1)",
        ]
    );
}

#[test]
fn recursive_and_function_definitions() {
    assert_eq!(
        units("RECURSIVE F(_), G\nF(n) == n\nf[x \\in S, y, z \\in T] == x\nLOCAL g[<<a, b>> \\in S] == a"),
        ["(recursive F/1 G)", "(def F (n) n)", "(fndef f [x \\in S] [y z \\in T] x)", "(local-fndef g [<<a b>> \\in S] a)",]
    );
}

#[test]
fn instances() {
    assert_eq!(
        units(
            "INSTANCE M\nLOCAL INSTANCE N WITH a <- 1, b <- x'\nI == INSTANCE M\n\
             J(p, Q(_)) == INSTANCE M WITH a <- p, + <- -, \\X <- \\times\nLOCAL K == INSTANCE M"
        ),
        [
            "(instance M)",
            "(local-instance N (a <- 1) (b <- (' x)))",
            "(instance I () M)",
            "(instance J (p Q/1) M (a <- p) (+ <- -) (\\X <- \\X))",
            "(local-instance K () M)",
        ]
    );
}

#[test]
fn assumptions() {
    assert_eq!(
        units("ASSUME a = 1\nASSUMPTION N == a \\in Nat\nAXIOM TRUE"),
        ["(assume (= a 1))", "(assume N (\\in a Nat))", "(assume TRUE)"]
    );
}

// --------------------------------------------------------------- literals

#[test]
fn numbers_and_strings() {
    assert_eq!(ex("<<0, 42, \\b101, \\B11, \\o17, \\O7, \\h1F, \\HfF, 1.50, .5>>"), "<<0 42 5 3 15 7 31 255 1.50 .5>>");
    assert_eq!(ex("170141183460469231731687303715884105727"), "170141183460469231731687303715884105727");
    match &module("X == \"a\\\"b\\\\c\\nd\\te\\rf\\fg\"").units[0] {
        Unit::OpDef(d) => match &*d.body.kind {
            ExprKind::Str(s) => assert_eq!(&**s, "a\"b\\c\nd\te\rf\u{c}g"),
            k => panic!("{k:?}"),
        },
        _ => panic!(),
    }
    assert_eq!(ex("\"(* not a comment *)\""), "\"(* not a comment *)\"");
}

#[test]
fn builtin_constants() {
    assert_eq!(ex("<<TRUE, FALSE, BOOLEAN, STRING, Nat, Int, Real>>"), "<<TRUE FALSE BOOLEAN STRING Nat Int Real>>");
    assert_eq!(ex("<<ℕ, ℤ, ℝ>>"), "<<Nat Int Real>>");
}

// ------------------------------------------------------------- operators

#[test]
fn infix_aliases_canonicalized() {
    let cases = [
        ("a \\union b", "(\\cup a b)"),
        ("a \\cup b", "(\\cup a b)"),
        ("a ∪ b", "(\\cup a b)"),
        ("a \\intersect b", "(\\cap a b)"),
        ("a ∩ b", "(\\cap a b)"),
        ("a =< b", "(<= a b)"),
        ("a \\leq b", "(<= a b)"),
        ("a ≤ b", "(<= a b)"),
        ("a \\geq b", "(>= a b)"),
        ("a ≥ b", "(>= a b)"),
        ("a # b", "(/= a b)"),
        ("a ≠ b", "(/= a b)"),
        ("a /= b", "(/= a b)"),
        ("a \\equiv b", "(<=> a b)"),
        ("a ≡ b", "(<=> a b)"),
        ("a ⇔ b", "(<=> a b)"),
        ("a ⇒ b", "(=> a b)"),
        ("a \\circ b", "(\\o a b)"),
        ("a ∘ b", "(\\o a b)"),
        ("a \\o b", "(\\o a b)"),
        ("a ∈ b", "(\\in a b)"),
        ("a ∉ b", "(\\notin a b)"),
        ("a ⊆ b", "(\\subseteq a b)"),
        ("a \\ b", "(\\ a b)"),
        ("a ~> b", "(~> a b)"),
        ("a ⇝ b", "(~> a b)"),
        ("a \\div b", "(\\div a b)"),
        ("a % b", "(% a b)"),
        ("a ^ b", "(^ a b)"),
        ("a .. b", "(.. a b)"),
        ("a ‥ b", "(.. a b)"),
        ("a :> b", "(:> a b)"),
        ("a @@ b", "(@@ a b)"),
        ("a \\cdot b", "(\\cdot a b)"),
        ("a ⋅ b", "(\\cdot a b)"),
        ("a ++ b", "(++ a b)"),
        ("a \\prec b", "(\\prec a b)"),
        ("a ≺ b", "(\\prec a b)"),
        ("a (+) b", "(\\oplus a b)"),
        ("a ⊕ b", "(\\oplus a b)"),
        ("a -+-> b", "(-+-> a b)"),
        ("a ⇸ b", "(-+-> a b)"),
        ("a ** b", "(** a b)"),
    ];
    for (src, want) in cases {
        assert_eq!(ex(src), want, "for {src}");
    }
}

#[test]
fn prefix_and_postfix() {
    assert_eq!(ex("~a"), "(~ a)");
    assert_eq!(ex("\\lnot a"), "(~ a)");
    assert_eq!(ex("\\neg a"), "(~ a)");
    assert_eq!(ex("¬a"), "(~ a)");
    assert_eq!(ex("-a"), "(-. a)");
    assert_eq!(ex("a - -3"), "(- a (-. 3))");
    assert_eq!(ex("x'"), "(' x)");
    assert_eq!(ex("f[x]'"), "(' (app f x))");
    assert_eq!(ex("r^+"), "(^+ r)");
    assert_eq!(ex("r⁺"), "(^+ r)");
    assert_eq!(
        ex("<<ENABLED A, UNCHANGED <<x, y>>, SUBSET S, UNION S, DOMAIN f>>"),
        "<<(ENABLED A) (UNCHANGED <<x y>>) (SUBSET S) (UNION S) (DOMAIN f)>>"
    );
    assert_eq!(ex("[]<>P"), "([] (<> P))");
    assert_eq!(ex("□◇P"), "([] (<> P))");
}

#[test]
fn nonfix_application() {
    assert_eq!(ex("+(1, 2)"), "(+ 1 2)");
    assert_eq!(ex("\\cup(a, b)"), "(\\cup a b)");
}

#[test]
fn precedence() {
    assert_eq!(ex("a + b * c"), "(+ a (* b c))");
    assert_eq!(ex("a - b - c"), "(- (- a b) c)");
    assert_eq!(ex("~a = b"), "(~ (= a b))");
    assert_eq!(ex("x \\in S \\cup T"), "(\\in x (\\cup S T))");
    assert_eq!(ex("a => b /\\ c"), "(=> a (/\\ b c))");
    assert_eq!(ex("-x^2"), "(-. (^ x 2))");
    assert_eq!(ex("(a + b) * c"), "(* (+ a b) c)");
    // A prefix operator before a parenthesized operand is still a prefix
    // operator (tree-sitter parses `~(a)` like a function application).
    assert_eq!(ex("~(a) = b"), "(~ (= a b))");
    assert_eq!(ex("~(a) /\\ b"), "(/\\ (~ a) b)");
    assert_eq!(ex("~(a).f"), "(~ (. a f))");
    assert_eq!(ex("[](a = 1).f"), "([] (. (= a 1) f))");
    assert_eq!(ex("SUBSET (a)[1]"), "(SUBSET (app a 1))");
    assert_eq!(ex("~(ENABLED A)'"), "(~ (' (ENABLED A)))");
}

/// SANY rejects operators with overlapping precedence ranges competing for
/// an operand (tla2sany/parser/OperatorStack.java).
#[test]
fn precedence_conflicts() {
    for e in [
        r"a /\ b \/ c",
        r"a \/ b /\ c",
        r"a = b = c",
        r"a = b # c",
        r"a \in S \in T",
        r"a \in S = b",
        r"a => b => c",
        r"a ~> b ~> c",
        r"a \cup b \cap c",
        r"a \div b \div c",
        r"a^b^c",
        r"UNCHANGED x'",
        r"[]x = 1",
        r"[](x = 1) = TRUE",
        r"ENABLED A = B",
        r"a /\ b = c \/ d",
    ] {
        let es = errors(&format!("X == {e}"));
        assert!(es.iter().any(|m| m.contains("Precedence conflict")), "{e}: {es:?}");
    }
    // Fine: same left-associative operator, disjoint ranges, bullet lists,
    // parentheses, prefix operators applied to prefix operators.
    for e in [
        r"a /\ b /\ c",
        r"a \cup b \cup c",
        r"a + b - c * d",
        r"a - b + c",
        r"a % 2 * 3",
        r"~a = b",
        r"SUBSET a \cup b",
        r"-a + b",
        r"x' = y",
        r"(a /\ b) \/ c",
        r"a = (b = c)",
        r"[]<>(x = 1) /\ <>[]P",
        r"~ENABLED A",
        r"A \cdot B \cdot C",
        r"a = 1 => b = 2 <=> c",
        "/\\ a \\/ b\n     /\\ c",
        "\\/ a /\\ b\n     \\/ c",
        r"x' \in S /\ UNCHANGED <<y>>",
    ] {
        module(&format!("X == {e}"));
    }
}

#[test]
fn cartesian_product() {
    assert_eq!(ex("A \\X B"), "(\\X A B)");
    assert_eq!(ex("A \\X B \\X C"), "(\\X A B C)");
    assert_eq!(ex("A \\times B × C \\X D"), "(\\X A B C D)");
    assert_eq!(ex("(A \\X B) \\X C"), "(\\X (\\X A B) C)");
    assert_eq!(ex("A \\X (B \\X C)"), "(\\X A (\\X B C))");
    assert_eq!(ex("x \\in A \\X B \\X C"), "(\\in x (\\X A B C))");
}

#[test]
fn junctions() {
    // Not flattened (as SANY sees them for subexpression selectors).
    assert_eq!(ex("a /\\ b /\\ c"), "(/\\ (/\\ a b) c)");
    assert_eq!(ex("a \\land b ∧ c"), "(/\\ (/\\ a b) c)");
    assert_eq!(ex("a \\/ b \\lor c ∨ d"), "(\\/ (\\/ (\\/ a b) c) d)");
    assert_eq!(ex("a /\\ (b /\\ c)"), "(/\\ a (/\\ b c))");
    assert_eq!(ex("a /\\ (b \\/ c)"), "(/\\ a (\\/ b c))");
    assert_eq!(ex("/\\ a\n     /\\ b"), "(/\\ a b)");
    assert_eq!(ex("\\/ a\n     \\/ b"), "(\\/ a b)");
    assert_eq!(
        ex("/\\ a\n     /\\ \\/ b\n        \\/ c\n     /\\ /\\ d\n        /\\ e\n     /\\ f /\\ g"),
        "(/\\ a (\\/ b c) (/\\ d e) (/\\ f g))"
    );
    assert_eq!(ex("∧ a\n     ∧ b"), "(/\\ a b)");
    assert_eq!(ex("/\\ a \\* comment\n     (* block *)\n     /\\ b"), "(/\\ a b)");
    // A single-item list is still a junction.
    assert_eq!(ex("/\\ a"), "(/\\ a)");
}

#[test]
fn operator_arguments() {
    assert_eq!(ex("SortSeq(s, <)"), "(SortSeq s <)");
    assert_eq!(ex("F(\\cup, \\union, -, -., ~, ')"), "(F \\cup \\cup - -. ~ ')");
    assert_eq!(ex("F(LAMBDA x, y : x + y, 1)"), "(F (LAMBDA [x y] (+ x y)) 1)");
    assert_eq!(ex("F(G(1), x)"), "(F (G 1) x)");
}

#[test]
fn instance_qualified_names() {
    assert_eq!(ex("I!Op"), "I!Op");
    assert_eq!(ex("I!Op(1, 2)"), "(I!Op 1 2)");
    assert_eq!(ex("I(x)!Op(y)"), "(I(x)!Op y)");
    assert_eq!(ex("M!N!Op"), "M!N!Op");
    assert_eq!(ex("I(a, b)!J!Op(c)"), "(I(a, b)!J!Op c)");
    let m = module("X == I(x)!Op(y)");
    match &*body(&m, 0).kind {
        ExprKind::OpAppl { prefix, name, args } => {
            assert_eq!(prefix.len(), 1);
            assert_eq!(&*prefix[0].name.name, "I");
            assert_eq!(prefix[0].args.len(), 1);
            assert_eq!(&*name.name, "Op");
            assert_eq!(args.len(), 1);
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn subexpression_selectors() {
    assert_eq!(ex("Inv!2"), "(subexpr Inv !2)");
    assert_eq!(ex("Thm!:"), "(subexpr Thm !:)");
    assert_eq!(ex("Inv!2!1"), "(subexpr Inv !2 !1)");
    assert_eq!(ex("Op(1, b)!2"), "(subexpr (Op 1 b) !2)");
    assert_eq!(ex("I(3)!Op(a)!2"), "(subexpr (I(3)!Op a) !2)");
    // Name components after a selector are labels.
    assert_eq!(ex("Inv!2!P0"), "(subexpr Inv !2 !P0)");
    assert_eq!(ex("I!Inv!P0!:"), "(subexpr I!Inv!P0 !:)");
    // Without a positional selector `Inv!P0` is syntactically an
    // instance-qualified name; the resolver tells the two apart.
    assert_eq!(ex("Inv!P0"), "Inv!P0");
    assert_eq!(ex("Inv!2 /\\ x"), "(/\\ (subexpr Inv !2) x)");
}

#[test]
fn subexpression_selectors_extended() {
    assert_eq!(ex("Op!<<"), "(subexpr Op !<<)");
    assert_eq!(ex("Op!>>!1"), "(subexpr Op !>> !1)");
    assert_eq!(ex("Op!(1)!@"), "(subexpr Op (! 1) !@)");
    assert_eq!(ex("Op!2!(1, b)"), "(subexpr Op !2 (! 1 b))");
    assert_eq!(ex("Inv!2!l(1)"), "(subexpr Inv !2 (!l 1))");
    // (A label path without a positional selector is resolved in sema.)
    assert_eq!(ex("Inv!l(1, 2)!:"), "(subexpr (Inv!l 1 2) !:)");
}

// ------------------------------------------------------------- binders

#[test]
fn quantifiers() {
    assert_eq!(ex("\\A x \\in S : P"), "(\\A [x \\in S] P)");
    assert_eq!(ex("\\E x, y \\in S, z \\in T : P"), "(\\E [x y \\in S] [z \\in T] P)");
    assert_eq!(ex("\\E <<x, y>> \\in S : P"), "(\\E [<<x y>> \\in S] P)");
    assert_eq!(ex("∀ x ∈ S : P"), "(\\A [x \\in S] P)");
    assert_eq!(ex("\\forall x : P"), "(\\A [x] P)");
    assert_eq!(ex("\\exists x, y : P"), "(\\E [x y] P)");
    assert_eq!(ex("∃ x : P"), "(\\E [x] P)");
    assert_eq!(ex("\\AA x : P"), "(\\AA [x] P)");
    assert_eq!(ex("\\EE x, y : P"), "(\\EE [x y] P)");
    assert!(matches!(
        *module("X == \\EE x : P")
            .units
            .iter()
            .find_map(|u| match u {
                Unit::OpDef(d) => Some(d.body.kind.clone()),
                _ => None,
            })
            .unwrap(),
        ExprKind::TemporalQuant { kind: QuantKind::Exists, .. }
    ));
}

#[test]
fn choose() {
    assert_eq!(ex("CHOOSE x \\in S : P"), "(CHOOSE [x \\in S] P)");
    assert_eq!(ex("CHOOSE x : P"), "(CHOOSE [x] P)");
    assert_eq!(ex("CHOOSE <<x, y>> \\in S : P"), "(CHOOSE [<<x y>> \\in S] P)");
}

#[test]
fn sets() {
    assert_eq!(ex("{}"), "(set)");
    assert_eq!(ex("{1, 2}"), "(set 1 2)");
    assert_eq!(ex("{x \\in S : x > 1}"), "(filter [x \\in S] (> x 1))");
    assert_eq!(ex("{<<x, y>> \\in S : x > y}"), "(filter [<<x y>> \\in S] (> x y))");
    assert_eq!(ex("{x + 1 : x \\in S}"), "(map (+ x 1) [x \\in S])");
    assert_eq!(ex("{<<x, y>> : x, y \\in S, z \\in T}"), "(map <<x y>> [x y \\in S] [z \\in T])");
    assert_eq!(ex("{x \\in S : y \\in T}"), "(filter [x \\in S] (\\in y T))");
}

#[test]
fn functions_tuples_records() {
    assert_eq!(ex("<<>>"), "<<>>");
    assert_eq!(ex("<<1, 2>>"), "<<1 2>>");
    assert_eq!(ex("⟨1, 2⟩"), "<<1 2>>");
    assert_eq!(ex("[x \\in S |-> x]"), "(fn [x \\in S] x)");
    assert_eq!(ex("[x, y \\in S, z \\in T ↦ x]"), "(fn [x y \\in S] [z \\in T] x)");
    assert_eq!(ex("f[a]"), "(app f a)");
    assert_eq!(ex("f[a, b]"), "(app f a b)");
    assert_eq!(ex("f[a][b]"), "(app (app f a) b)");
    assert_eq!(ex("[S -> T]"), "(-> S T)");
    assert_eq!(ex("[S → T]"), "(-> S T)");
    assert_eq!(ex("[a |-> 1, b |-> 2]"), "(rec (a 1) (b 2))");
    assert_eq!(ex("[a : S, b : T]"), "(recset (a S) (b T))");
    assert_eq!(ex("r.a.b"), "(. (. r a) b)");
    assert_eq!(ex("f[x].a"), "(. (app f x) a)");
}

#[test]
fn nat_int_real_as_record_fields() {
    // Ordinary names in TLA+ (the grammar has keywords for them): SANY
    // accepts them as record fields, with a lint warning.
    assert_eq!(ex("[Nat |-> 1, b |-> Nat]"), "(rec (Nat 1) (b Nat))");
    assert_eq!(ex("[a : Nat, Int : [Real |-> 2]]"), "(recset (a Nat) (Int (rec (Real 2))))");
    assert_eq!(ex("r.Nat + r . Int"), "(+ (. r Nat) (. r Int))");
    assert_eq!(ex("[r EXCEPT !.Real = 1]"), "(except r ((.Real) 1))");
    // Elsewhere they stay the sets.
    assert_eq!(ex("[n \\in Nat |-> n]"), "(fn [n \\in Nat] n)");
    assert_eq!(ex("{x \\in Int : x \\in 1..Nat}"), "(filter [x \\in Int] (\\in x (.. 1 Nat)))");
}

#[test]
fn nested_pair_literals() {
    // `<<` and `>>` are single tokens, so a nested pair literal needs the
    // inner one closed before the next comma: `<<<<a, b>, c>, d>>` (the
    // form one might first write) is not valid TLA+ -- SANY and TLC both
    // reject it with a parse error -- and tlx rejects it too (FE-8). The
    // valid spellings double the inner brackets.
    assert_eq!(ex("<<<<0, 1>>, 2>>"), "<<<<0 1>> 2>>");
    assert_eq!(ex("<<<<<<0, 1>>, 2>>, 3>>"), "<<<<<<0 1>> 2>> 3>>");
    let errs = errors("X == <<<<0, 1>, 2>, 3>>");
    assert!(errs.iter().all(|e| e.contains("syntax error")), "{errs:?}");
}

#[test]
fn except() {
    assert_eq!(ex("[f EXCEPT ![1] = 2]"), "(except f (([1]) 2))");
    assert_eq!(ex("[f EXCEPT ![a].b[c, d] = @ + 1, !.e = @]"), "(except f (([a] .b [c d]) (+ @ 1)) ((.e) @))");
}

#[test]
fn control() {
    assert_eq!(ex("IF a THEN b ELSE c"), "(IF a b c)");
    assert_eq!(ex("CASE a -> 1 [] b -> 2"), "(CASE (a 1) (b 2))");
    assert_eq!(ex("CASE a -> 1 □ OTHER -> 3"), "(CASE (a 1) (OTHER 3))");
    assert_eq!(
        ex("LET a == 1\n       f[x \\in S] == x\n       RECURSIVE g(_)\n       g(n) == n\n       I == INSTANCE M\n   IN a"),
        "(LET [(def a () 1) (fndef f [x \\in S] x) (recursive g/1) (def g (n) n) (instance I () M)] a)"
    );
}

#[test]
fn labels_kept() {
    assert_eq!(ex("l :: a + 1"), "(label l (+ a 1))");
    // Label parameters are kept; a labeled junction is not spliced.
    assert_eq!(ex("/\\ l1 :: a\n     /\\ l2(x) :: b"), "(/\\ (label l1 a) (label l2(x) b))");
    assert_eq!(ex("/\\ a\n     /\\ l :: /\\ b\n             /\\ c"), "(/\\ a (label l (/\\ b c)))");
}

#[test]
fn actions_and_temporal() {
    assert_eq!(ex("[Next]_vars"), "([]_ Next vars)");
    assert_eq!(ex("[][Next]_<<x, y>>"), "([] ([]_ Next <<x y>>))");
    assert_eq!(ex("<<A>>_v"), "(<>_ A v)");
    assert_eq!(ex("<>⟨A⟩_v"), "(<> (<>_ A v))");
    assert_eq!(ex("WF_vars(Next)"), "(WF vars Next)");
    assert_eq!(ex("SF_<<x, y>>(A(1))"), "(SF <<x y>> (A 1))");
    assert_eq!(ex("WF_x(A) /\\ SF_x(B)"), "(/\\ (WF x A) (SF x B))");
    assert_eq!(ex("P ~> Q"), "(~> P Q)");
}

// -------------------------------------------------------------- errors

#[test]
fn syntax_errors_reported_with_positions() {
    let errs = errors("A == 1\nB == (1 +\nC == 2");
    assert!(!errs.is_empty());
    assert!(errs.iter().all(|e| e.contains("syntax error")), "{errs:?}");
    // Errors are reported at a line inside the module, 1-based.
    assert!(errs.iter().all(|e| e.split(':').next().unwrap().parse::<u32>().unwrap() >= 2), "{errs:?}");

    let errs = errors("A == 1\nB == IF a THEN 1\nC == [x \\in S |-> ]");
    assert!(errs.len() >= 2, "{errs:?}");
}

#[test]
fn no_module() {
    assert!(parse_module("X == 1\n", FileId(0)).is_err());
}

#[test]
fn errors_in_pluscal_comments_ignored() {
    let u = units("(* --algorithm a\nbegin\n  A: x := ;;; )( \nend algorithm *)\nX == 1");
    assert_eq!(u, ["(def X () 1)"]);
}

// --------------------------------------------------------------- spans

#[test]
fn spans() {
    let src = "---- MODULE T ----\nVARIABLE x\nNext == x' = x + 1\n====\n";
    let m = parse_module(src, FileId(5)).unwrap();
    let Unit::OpDef(d) = &m.units[1] else { panic!() };
    assert_eq!((d.name.span.line, d.name.span.col), (3, 1));
    let e = &d.body;
    assert_eq!(&src[e.span.start as usize..e.span.end as usize], "x' = x + 1");
    assert_eq!((e.span.line, e.span.col, e.span.file), (3, 9, FileId(5)));
    let ExprKind::OpAppl { name, args, .. } = &*e.kind else { panic!() };
    assert_eq!(&*name.name, "=");
    assert_eq!(&src[name.span.start as usize..name.span.end as usize], "=");
    assert_eq!((name.span.line, name.span.col), (3, 12));
    assert_eq!(&src[args[1].span.start as usize..args[1].span.end as usize], "x + 1");
    let ExprKind::OpAppl { name: prime, args: pargs, .. } = &*args[0].kind else { panic!() };
    assert_eq!(&*prime.name, "'");
    assert_eq!(pargs.len(), 1);
    let Unit::Variables(vs) = &m.units[0] else { panic!() };
    assert_eq!((vs[0].span.line, vs[0].span.col, vs[0].span.start), (2, 10, 28));
}

#[test]
fn tabs_align_like_sany() {
    // SANY advances a tab to the next multiple-of-8 column: the inner list's
    // second bullet (two tabs + 2 spaces = column 19) is aligned with the
    // first one (`\t/\ \/ ` puts it at column 12 with one-column tabs).
    let src = "---- MODULE T ----\nA ==\n\t/\\ 1 = 1\n\t/\\ \\/ 2 = 2\n\t   \\/ \"\t\"\n====\n";
    let m = parse_module(src, FileId(0)).unwrap();
    let Unit::OpDef(d) = &m.units[0] else { panic!() };
    assert_eq!(sexp::expr(&d.body), "(/\\ (= 1 1) (\\/ (= 2 2) \"\\t\"))");
    // Spans refer to the original text (tabs count as one column).
    let ExprKind::And(es) = &*d.body.kind else { panic!() };
    assert_eq!((es[1].span.line, es[1].span.col), (4, 5));
    assert_eq!(src[es[1].span.start as usize..es[1].span.end as usize].trim_end(), "\\/ 2 = 2\n\t   \\/ \"\t\"");
}

#[test]
fn operator_symbols_in_subexpression_paths() {
    // A symbol ending the path (the grammar only accepts `D!++(a, b)`).
    assert_eq!(ex("F(D!@!++, I!\\oplus)"), "(F (subexpr D !@ !++) I!\\oplus)");
    assert_eq!(ex("D!(1)!++(2, 3)"), "(subexpr D (! 1) (!++ 2 3))");
    // Inside strings nothing changes.
    assert_eq!(ex("\"D!++\""), "\"D!++\"");
}

#[test]
fn proofs_are_kept() {
    let m = module("THEOREM T == ASSUME NEW c PROVE c = c\n<1>1. c = c\n<*> QED BY <1>1 DEF T\nUSE T");
    assert!(m.units.is_empty());
    assert_eq!(m.proofs.len(), 2);
    let (_, ProofUnit::Theorem { stmt: Stmt::AssumeProve(ap), proof: Some(Proof::Steps(steps)), .. }) = &m.proofs[0] else {
        panic!("{:?}", m.proofs[0])
    };
    assert!(matches!(&ap.assumes[0], Assumption::New(_)));
    assert_eq!(&*steps[0].id.name, "<1>1");
    assert!(matches!(steps[1].kind, StepKind::Qed));
    assert!(errors("THEOREM TRUE\n<*>a TRUE\n<*> QED")[0].contains("<*> and <+> cannot be used for a named step."));
}

#[test]
fn binder_bodies_extend_right() {
    assert_eq!(ex("\\E x \\in S : P /\\ Q"), "(\\E [x \\in S] (/\\ P Q))");
    assert_eq!(ex("\\A x : P => Q"), "(\\A [x] (=> P Q))");
    assert_eq!(ex("IF a THEN b ELSE c + 1"), "(IF a b (+ c 1))");
    assert_eq!(ex("LET y == 1 IN y + 1"), "(LET [(def y () 1)] (+ y 1))");
    assert_eq!(ex("CHOOSE x \\in S : x > 1 /\\ x < 3"), "(CHOOSE [x \\in S] (/\\ (> x 1) (< x 3)))");
}

#[test]
fn junction_list_ends_at_outdent() {
    assert_eq!(ex("/\\ a\n     /\\ b\n   => c"), "(=> (/\\ a b) c)");
    assert_eq!(units("A == /\\ a\n     /\\ \\/ b\n        \\/ c\nB == 1"), ["(def A () (/\\ a (\\/ b c)))", "(def B () 1)"]);
}

#[test]
fn misc_forms() {
    assert_eq!(ex("I!+(1, 2)"), "(I!+ 1 2)");
    assert_eq!(ex("I!Nat"), "I!Nat");
    assert_eq!(ex("x.a'"), "(' (. x a))");
    assert_eq!(ex("[]A /\\ ENABLED B"), "(/\\ ([] A) (ENABLED B))");
    assert_eq!(units("X ≜ 1"), ["(def X () 1)"]);
    assert_eq!(units("I == INSTANCE M WITH F <- LAMBDA x : x"), ["(instance I () M (F <- (LAMBDA [x] x)))"]);
}
