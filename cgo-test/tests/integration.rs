#[test]
fn test_add() {
    unsafe {
        let out = cgo_test::add(1, 2);
        assert_eq!(out, 3);
    }
}
