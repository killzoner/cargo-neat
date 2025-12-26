#[test]
fn ui() {
    let t = trycmd::TestCases::new();
    t.case("tests/ui/*/*.toml");
}
