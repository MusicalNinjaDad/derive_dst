// The `.stderr` files are recorded with native `proc_macro_diagnostic` output (nightly);
// on toolchains without it the fallback rendering differs, so only run there.

#[cfg(not(miri))]
#[test]
fn errors() {
    use trybuild::TestCases;

    TestCases::new().compile_fail("tests/compilation/fail_*.rs");
}
